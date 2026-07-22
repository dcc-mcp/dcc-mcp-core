use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;

use dcc_mcp_ui_control::host_protocol::{
    UI_CONTROL_HOST_MAX_FRAME_BYTES, UI_CONTROL_HOST_PROTOCOL_VERSION, UiControlHostErrorCode,
    UiControlHostRequest, UiControlHostResponse,
};
use semver::Version;
use sha2::{Digest, Sha256};
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
const MAX_HOST_VERSION_CHARS: usize = 64;
const MAX_NAMED_PIPE_CHARS: usize = 256;

struct DiscoveryEndpoints {
    pipe: String,
    singleton: String,
}

fn canonical_endpoint_version(version: &str) -> Result<String, String> {
    if version.len() > MAX_HOST_VERSION_CHARS {
        return Err("UI Control host version is too long for discovery".to_owned());
    }
    let canonical = Version::parse(version)
        .map_err(|_| "UI Control host version is not strict SemVer".to_owned())?
        .to_string();
    if canonical != version
        || !canonical
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return Err("UI Control host version is not canonical safe SemVer".to_owned());
    }
    Ok(canonical)
}

fn validate_binary_identity(sha256: &str) -> Result<(), String> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err("UI Control host discovery requires a full lowercase SHA-256".to_owned());
    }
    Ok(())
}

fn binary_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("open the current UI Control host image: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("read the current UI Control host image: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = digest.finalize();
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    Ok(encoded)
}

fn discovery_endpoints(
    session_id: u32,
    version: &str,
    sha256: &str,
) -> Result<DiscoveryEndpoints, String> {
    let version = canonical_endpoint_version(version)?;
    validate_binary_identity(sha256)?;
    let suffix = format!(
        "v{UI_CONTROL_HOST_PROTOCOL_VERSION}-version-{version}-sha256-{sha256}-session-{session_id}"
    );
    let pipe = format!(r"\\.\pipe\dcc-mcp-ui-control-host-{suffix}");
    if pipe.encode_utf16().count() >= MAX_NAMED_PIPE_CHARS {
        return Err("UI Control host discovery pipe name is too long".to_owned());
    }
    Ok(DiscoveryEndpoints {
        pipe,
        singleton: format!(r"Local\dcc-mcp-ui-control-host-{suffix}"),
    })
}

pub(super) fn run() -> Result<(), String> {
    let session_id = current_session_id()?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("resolve the current UI Control host image: {error}"))?;
    let sha256 = binary_sha256(&executable)?;
    let endpoints = discovery_endpoints(session_id, env!("CARGO_PKG_VERSION"), &sha256)?;
    let security = OwnerOnlySecurity::new()?;
    let _singleton = acquire_singleton(&endpoints.singleton, security.attributes())?;
    let pipe_name = wide(&endpoints.pipe);
    // Validate operator configuration before accepting clients. Each named-pipe connection then
    // owns an independent host state machine, so a long capture in one DCC session cannot hold a
    // process-wide Rust mutex and block observations in another session. Native input remains
    // globally serialized by the per-Windows-session input-owner mutex.
    let _ = UiControlHost::from_operator_config()?;

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
        let state = match UiControlHost::from_operator_config() {
            Ok(state) => state,
            Err(error) => {
                eprintln!("UI Control host could not create connection state: {error}");
                continue;
            }
        };
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
    name: &str,
    security: &SECURITY_ATTRIBUTES,
) -> Result<OwnedKernelHandle, String> {
    let name = wide(name);
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

fn serve_client(mut file: File, mut host: UiControlHost) {
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
        let response = connection.handle(&mut host, request);
        if write_response(&mut file, &response).is_err() {
            break;
        }
    }
    let handle = HANDLE(file.as_raw_handle());
    let _ = unsafe { FlushFileBuffers(handle) };
    let _ = unsafe { DisconnectNamedPipe(handle) };
    connection.disconnect(&mut host);
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

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn binary_identity_uses_full_standard_sha256() {
        let path = std::env::temp_dir().join(format!(
            "dcc-mcp-ui-control-host-sha256-test-{}.bin",
            std::process::id()
        ));
        std::fs::write(&path, b"abc").unwrap();

        let digest = binary_sha256(&path).unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn discovery_identity_contains_exact_version_and_full_binary_sha256() {
        let endpoints = discovery_endpoints(42, "0.19.65", DIGEST_A).unwrap();

        assert_eq!(
            endpoints.pipe,
            format!(
                r"\\.\pipe\dcc-mcp-ui-control-host-v3-version-0.19.65-sha256-{DIGEST_A}-session-42"
            )
        );
        assert_eq!(
            endpoints.singleton,
            format!(
                r"Local\dcc-mcp-ui-control-host-v3-version-0.19.65-sha256-{DIGEST_A}-session-42"
            )
        );
        assert!(endpoints.pipe.encode_utf16().count() < MAX_NAMED_PIPE_CHARS);
        assert!(!endpoints.pipe.ends_with("v3-session-42"));
    }

    #[test]
    fn discovery_identity_rejects_unsafe_or_noncanonical_values() {
        for version in [
            "01.19.65",
            "0.19.65-01",
            "0.19.65\\other",
            "0.19.65/other",
            "v0.19.65",
        ] {
            assert!(discovery_endpoints(42, version, DIGEST_A).is_err());
        }
        assert!(discovery_endpoints(42, "0.19.65", &"a".repeat(63)).is_err());
        assert!(discovery_endpoints(42, "0.19.65", &"A".repeat(64)).is_err());
    }

    #[test]
    fn singleton_is_exclusive_per_version_and_digest_identity() {
        let session_id = 0x8000_0000 | std::process::id();
        let security = OwnerOnlySecurity::new().unwrap();
        let first_name = discovery_endpoints(session_id, "0.19.65", DIGEST_A)
            .unwrap()
            .singleton;
        let other_digest_name = discovery_endpoints(session_id, "0.19.65", DIGEST_B)
            .unwrap()
            .singleton;
        let _first = acquire_singleton(&first_name, security.attributes()).unwrap();

        let duplicate_error = match acquire_singleton(&first_name, security.attributes()) {
            Ok(_) => panic!("the same Host identity must remain a singleton"),
            Err(error) => error,
        };
        assert!(duplicate_error.contains("already running"));
        let _other_digest = acquire_singleton(&other_digest_name, security.attributes()).unwrap();
    }
}
