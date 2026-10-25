//! Live file identity for retrying failed prepared-file consumption.
//!
//! The handle stays open across the operation, preventing identifier reuse.
//! This complements the installation lock; it is not an atomic filesystem
//! sandbox against writers that ignore that lock.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub(super) struct PreparedIdentity {
    file: File,
}

impl PreparedIdentity {
    pub(super) fn capture(path: &Path) -> io::Result<Self> {
        let file = open_regular(path)?;
        // Refuse unsupported identity queries before relinquishing ownership.
        file_identity(&file)?;
        Ok(Self { file })
    }

    pub(super) fn still_at(&self, path: &Path) -> io::Result<bool> {
        let current = match open_regular(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(file_identity(&self.file)? == file_identity(&current)?)
    }
}

fn open_regular(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        // The guard must not itself prevent the consuming rename/delete.
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other(
                "prepared identity refuses a reparse point",
            ));
        }
    }
    if !metadata.is_file() {
        return Err(io::Error::other(
            "prepared identity requires a regular file",
        ));
    }
    Ok(file)
}

#[cfg(unix)]
fn file_identity(file: &File) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn file_identity(file: &File) -> io::Result<(u64, [u8; 16])> {
    use std::mem::{MaybeUninit, size_of};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    let mut info = MaybeUninit::<FILE_ID_INFO>::uninit();
    // SAFETY: the borrowed live handle remains valid for this call, and the
    // writable buffer has exactly the size required by FileIdInfo. Read it only
    // after success. Unsupported filesystems/Windows versions fail closed.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            info.as_mut_ptr().cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful API call initialized FILE_ID_INFO above.
    let info = unsafe { info.assume_init() };
    Ok((info.VolumeSerialNumber, info.FileId.Identifier))
}

#[cfg(not(any(unix, windows)))]
fn file_identity(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "prepared file identity is unsupported on this platform",
    ))
}
