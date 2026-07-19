use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::{Arc, Mutex};

use dcc_mcp_app_ui::host_protocol::{
    UI_CONTROL_HOST_MAX_FRAME_BYTES, UiControlHostErrorCode, UiControlHostRequest,
    UiControlHostResponse,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, HLOCAL,
    LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, PSECURITY_DESCRIPTOR,
    SECURITY_ATTRIBUTES, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER, TokenIntegrityLevel,
    TokenUser,
};
use windows::Win32::Storage::FileSystem::{FlushFileBuffers, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    GetNamedPipeClientSessionId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::core::{PCWSTR, PWSTR};

use super::{UiControlHost, UiControlHostConnection};

const PIPE_BUFFER_BYTES: u32 = 64 * 1024;

pub(super) fn run() -> Result<(), String> {
    let session_id = current_session_id()?;
    let security = OwnerOnlySecurity::new()?;
    let _singleton = acquire_singleton(session_id, security.attributes())?;
    let pipe_name = wide(&format!(
        r"\\.\pipe\dcc-mcp-ui-control-host-v1-session-{session_id}"
    ));
    let host = Arc::new(Mutex::new(UiControlHost::default()));

    loop {
        let handle = create_pipe(&pipe_name, security.attributes())?;
        connect_pipe(handle)?;
        if let Err(error) = validate_client_session(handle, session_id)
            .and_then(|()| validate_client_identity(handle))
        {
            eprintln!("UI Control host rejected a named-pipe client: {error}");
            continue;
        }
        let file = handle_into_file(handle);
        let state = Arc::clone(&host);
        std::thread::spawn(move || serve_client(file, state));
    }
}

fn current_session_id() -> Result<u32, String> {
    let mut session_id = 0;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id) }
        .map_err(|error| format!("resolve the interactive Windows session: {error}"))?;
    Ok(session_id)
}

fn acquire_singleton(
    session_id: u32,
    security: &SECURITY_ATTRIBUTES,
) -> Result<OwnedKernelHandle, String> {
    let name = wide(&format!(
        r"Local\dcc-mcp-ui-control-host-v1-session-{session_id}"
    ));
    let handle = unsafe { CreateMutexW(Some(security), false, PCWSTR(name.as_ptr())) }
        .map_err(|error| format!("create the per-session host mutex: {error}"))?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let _ = unsafe { CloseHandle(handle) };
        return Err("a UI Control host is already running in this Windows session".to_owned());
    }
    Ok(OwnedKernelHandle(handle))
}

fn create_pipe(name: &[u16], security: &SECURITY_ATTRIBUTES) -> Result<HANDLE, String> {
    let mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            mode,
            PIPE_UNLIMITED_INSTANCES,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            Some(security),
        )
    };
    if handle.is_invalid() {
        return Err(format!(
            "create the owner-only UI Control named pipe: {}",
            windows::core::Error::from_thread()
        ));
    }
    Ok(handle)
}

fn connect_pipe(handle: HANDLE) -> Result<(), String> {
    if let Err(error) = unsafe { ConnectNamedPipe(handle, None) }
        && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED
    {
        let _ = unsafe { CloseHandle(handle) };
        return Err(format!("accept a UI Control client: {error}"));
    }
    Ok(())
}

fn validate_client_session(handle: HANDLE, expected_session_id: u32) -> Result<(), String> {
    let mut client_session_id = u32::MAX;
    if let Err(error) = unsafe { GetNamedPipeClientSessionId(handle, &mut client_session_id) } {
        let _ = unsafe { CloseHandle(handle) };
        return Err(format!("verify the UI Control client session: {error}"));
    }
    if client_session_id != expected_session_id {
        let _ = unsafe { CloseHandle(handle) };
        return Err("UI Control client is outside the host Windows session".to_owned());
    }
    Ok(())
}

fn validate_client_identity(handle: HANDLE) -> Result<(), String> {
    let mut client_process_id = 0;
    if let Err(error) = unsafe { GetNamedPipeClientProcessId(handle, &mut client_process_id) } {
        let _ = unsafe { CloseHandle(handle) };
        return Err(format!("resolve the UI Control client process: {error}"));
    }
    let client_process =
        match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, client_process_id) } {
            Ok(process) => process,
            Err(error) => {
                let _ = unsafe { CloseHandle(handle) };
                return Err(format!("inspect the UI Control client process: {error}"));
            }
        };
    let result = (|| {
        let host_identity = process_token_identity(unsafe { GetCurrentProcess() })?;
        let client_identity = process_token_identity(client_process)?;
        if client_identity != host_identity {
            return Err(
                "UI Control client user or integrity level differs from the host process"
                    .to_owned(),
            );
        }
        Ok(())
    })();
    let _ = unsafe { CloseHandle(client_process) };
    if result.is_err() {
        let _ = unsafe { CloseHandle(handle) };
    }
    result
}

fn process_token_identity(process: HANDLE) -> Result<(String, u32), String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }
        .map_err(|error| format!("open a UI Control process token: {error}"))?;
    let result = (|| {
        let user_buffer = token_information(token, TokenUser)?;
        let user = unsafe { &*(user_buffer.as_ptr().cast::<TOKEN_USER>()) };
        let mut string_sid = PWSTR::null();
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut string_sid) }
            .map_err(|error| format!("resolve the UI Control token user: {error}"))?;
        let user_sid = unsafe { string_sid.to_string() }
            .map_err(|error| format!("decode the UI Control token user: {error}"))?;
        let _ = unsafe { LocalFree(Some(HLOCAL(string_sid.0.cast()))) };

        let integrity_buffer = token_information(token, TokenIntegrityLevel)?;
        let label = unsafe { &*(integrity_buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()) };
        let count = unsafe { *GetSidSubAuthorityCount(label.Label.Sid) };
        if count == 0 {
            return Err("UI Control token has no integrity-level SID authority".to_owned());
        }
        let integrity = unsafe { *GetSidSubAuthority(label.Label.Sid, u32::from(count - 1)) };
        Ok((user_sid, integrity))
    })();
    let _ = unsafe { CloseHandle(token) };
    result
}

fn token_information(
    token: HANDLE,
    class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<Vec<usize>, String> {
    let mut required = 0;
    let _ = unsafe { GetTokenInformation(token, class, None, 0, &mut required) };
    if required == 0 {
        return Err("Windows returned an empty UI Control token identity".to_owned());
    }
    let words = usize::try_from(required)
        .unwrap_or(usize::MAX)
        .div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; words];
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            required,
            &mut required,
        )
    }
    .map_err(|error| format!("read a UI Control process token: {error}"))?;
    Ok(buffer)
}

fn handle_into_file(handle: HANDLE) -> File {
    unsafe { File::from_raw_handle(handle.0) }
}

fn serve_client(mut file: File, host: Arc<Mutex<UiControlHost>>) {
    let mut connection = UiControlHostConnection::default();
    loop {
        let request = match read_request(&mut file) {
            Ok(Some(request)) => request,
            Ok(None) => break,
            Err(message) => {
                let _ = write_response(
                    &mut file,
                    &UiControlHostResponse::Error {
                        code: UiControlHostErrorCode::InvalidRequest,
                        message,
                    },
                );
                break;
            }
        };
        let response = {
            let mut state = host.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            connection.handle(&mut state, request)
        };
        if write_response(&mut file, &response).is_err() {
            break;
        }
    }
    let handle = HANDLE(file.as_raw_handle());
    let _ = unsafe { FlushFileBuffers(handle) };
    let _ = unsafe { DisconnectNamedPipe(handle) };
    let mut state = host.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    connection.disconnect(&mut state);
}

fn read_request(file: &mut File) -> Result<Option<UiControlHostRequest>, String> {
    let mut prefix = [0_u8; 4];
    match file.read_exact(&mut prefix) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("read the UI Control frame length: {error}")),
    }
    let length = u32::from_be_bytes(prefix);
    if length == 0 || length > UI_CONTROL_HOST_MAX_FRAME_BYTES {
        return Err(format!(
            "UI Control JSON frame length {length} is outside 1..={UI_CONTROL_HOST_MAX_FRAME_BYTES}"
        ));
    }
    let mut body = vec![0_u8; length as usize];
    file.read_exact(&mut body)
        .map_err(|error| format!("read the UI Control JSON frame: {error}"))?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| format!("decode the versioned UI Control JSON request: {error}"))
}

fn write_response(file: &mut File, response: &UiControlHostResponse) -> Result<(), String> {
    let body = serde_json::to_vec(response)
        .map_err(|error| format!("encode the UI Control JSON response: {error}"))?;
    let length =
        u32::try_from(body.len()).map_err(|_| "UI Control response is too large".to_owned())?;
    if length > UI_CONTROL_HOST_MAX_FRAME_BYTES {
        return Err("UI Control response exceeds the frame limit".to_owned());
    }
    file.write_all(&length.to_be_bytes())
        .and_then(|_| file.write_all(&body))
        .and_then(|_| file.flush())
        .map_err(|error| format!("write the UI Control JSON response: {error}"))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

struct OwnerOnlySecurity {
    descriptor: PSECURITY_DESCRIPTOR,
    attributes: SECURITY_ATTRIBUTES,
}

impl OwnerOnlySecurity {
    fn new() -> Result<Self, String> {
        // Protected DACL: LocalSystem and the object owner only. The owner is
        // the current user token that creates the pipe and singleton mutex.
        let sddl = wide("D:P(A;;GA;;;SY)(A;;GA;;;OW)");
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|error| format!("build the owner-only pipe DACL: {error}"))?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        Ok(Self {
            descriptor,
            attributes,
        })
    }

    fn attributes(&self) -> &SECURITY_ATTRIBUTES {
        &self.attributes
    }
}

impl Drop for OwnerOnlySecurity {
    fn drop(&mut self) {
        let _ = unsafe { LocalFree(Some(HLOCAL(self.descriptor.0))) };
    }
}

struct OwnedKernelHandle(HANDLE);

impl Drop for OwnedKernelHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}
