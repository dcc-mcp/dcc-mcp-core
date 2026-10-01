//! Portable ZIP and 7z extraction for the existing archive install channel.
//!
//! Validate every member before extraction. Windows archives must not create
//! links, drive-qualified paths, alternate streams, or paths outside staging.

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::install::InstallError;

const TOOL_TIMEOUT: Duration = Duration::from_secs(300);
const LIST_LIMIT: u64 = 16 * 1024 * 1024;

fn failure(detail: impl Into<String>) -> InstallError {
    InstallError::ExtractionFailed {
        detail: detail.into(),
    }
}

/// Check portable archive paths independently of the current OS's separators.
fn member_path(name: &str, strip: usize) -> Result<Option<PathBuf>, InstallError> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.chars().any(char::is_control)
    {
        return Err(failure(format!("unsafe archive path: {name}")));
    }
    let parts: Vec<_> = normalized.trim_end_matches('/').split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == ".." || part.ends_with(['.', ' ']))
    {
        return Err(failure(format!("unsafe archive path: {name}")));
    }
    if parts.len() <= strip {
        return Ok(None);
    }
    Ok(Some(parts.into_iter().skip(strip).collect()))
}

pub(super) fn unpack_zip(
    bytes: &[u8],
    destination: &Path,
    strip: usize,
) -> Result<(), InstallError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|err| failure(err.to_string()))?;
    // Preflight the whole archive before the first member is written.
    for index in 0..archive.len() {
        let member = archive
            .by_index(index)
            .map_err(|err| failure(err.to_string()))?;
        member_path(member.name(), strip)?;
        if member
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(failure(format!(
                "archive links are not supported: {}",
                member.name()
            )));
        }
    }
    for index in 0..archive.len() {
        let mut member = archive
            .by_index(index)
            .map_err(|err| failure(err.to_string()))?;
        let Some(relative) = member_path(member.name(), strip)? else {
            continue;
        };
        let target = destination.join(relative);
        if member.is_dir() {
            std::fs::create_dir_all(target).map_err(|err| failure(err.to_string()))?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|err| failure(err.to_string()))?;
            }
            let mut file = std::fs::File::create(target).map_err(|err| failure(err.to_string()))?;
            std::io::copy(&mut member, &mut file).map_err(|err| failure(err.to_string()))?;
            #[cfg(unix)]
            if let Some(mode) = member.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(mode & 0o777))
                    .map_err(|err| failure(err.to_string()))?;
            }
        }
    }
    Ok(())
}

/// The installed 7-Zip parser handles BCJ/LZMA filters used by upstream builds.
/// List and validate all members before invoking extraction into private staging.
pub(super) fn unpack_7z(
    bytes: &[u8],
    destination: &Path,
    strip: usize,
) -> Result<(), InstallError> {
    let workspace = tempfile::tempdir_in(destination).map_err(|err| failure(err.to_string()))?;
    let archive = workspace.path().join("host.7z");
    std::fs::write(&archive, bytes).map_err(|err| failure(err.to_string()))?;
    let mut listing = tempfile::tempfile().map_err(|err| failure(err.to_string()))?;
    run_7z(
        &[
            "l".into(),
            "-slt".into(),
            "-ba".into(),
            "-sccUTF-8".into(),
            archive.as_os_str().to_owned(),
        ],
        listing
            .try_clone()
            .map_err(|err| failure(err.to_string()))?,
    )?;
    if listing
        .metadata()
        .map_err(|err| failure(err.to_string()))?
        .len()
        > LIST_LIMIT
    {
        return Err(failure("7z member listing exceeds the validation limit"));
    }
    listing.rewind().map_err(|err| failure(err.to_string()))?;
    let mut text = String::new();
    listing
        .read_to_string(&mut text)
        .map_err(|err| failure(err.to_string()))?;
    validate_listing(&text)?;

    let raw = workspace.path().join("unpacked");
    std::fs::create_dir(&raw).map_err(|err| failure(err.to_string()))?;
    let output_option = format!("-o{}", raw.display());
    run_7z(
        &[
            "x".into(),
            "-y".into(),
            "-bd".into(),
            "-bb0".into(),
            output_option.into(),
            archive.as_os_str().to_owned(),
        ],
        tempfile::tempfile().map_err(|err| failure(err.to_string()))?,
    )?;
    move_members(&raw, &raw, destination, strip)?;
    Ok(())
}

fn validate_listing(listing: &str) -> Result<(), InstallError> {
    let mut members = 0;
    for line in listing.lines() {
        if let Some(name) = line.strip_prefix("Path = ") {
            member_path(name, 0)?;
            members += 1;
        }
        if line.starts_with("Symbolic Link = ")
            || line.starts_with("Hard Link = ")
            || line.starts_with("Alternate Stream = ")
            || line
                .strip_prefix("Attributes = ")
                .is_some_and(|value| value.contains('l') || value.contains('L'))
        {
            return Err(failure(
                "archive links and alternate streams are not supported",
            ));
        }
    }
    if members == 0 {
        return Err(failure("7z returned no member paths to validate"));
    }
    Ok(())
}

fn run_7z(arguments: &[std::ffi::OsString], output: std::fs::File) -> Result<(), InstallError> {
    let mut command = Command::new("7z");
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(output.try_clone().map_err(|err| failure(err.to_string()))?)
        .stdout(output);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|err| {
        failure(format!(
            "7z must be installed and on PATH to unpack this archive: {err}"
        ))
    })?;
    let deadline = Instant::now() + TOOL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(failure(format!("7z exited with {status}"))),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure(match result {
                    Err(err) => err.to_string(),
                    _ => format!("7z exceeded {} seconds", TOOL_TIMEOUT.as_secs()),
                }));
            }
        }
    }
}

fn move_members(
    root: &Path,
    current: &Path,
    destination: &Path,
    strip: usize,
) -> Result<(), InstallError> {
    for item in std::fs::read_dir(current).map_err(|err| failure(err.to_string()))? {
        let item = item.map_err(|err| failure(err.to_string()))?;
        let path = item.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|err| failure(err.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(failure("7z produced an archive link"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(failure("7z produced a reparse point"));
            }
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|err| failure(err.to_string()))?;
        let target =
            member_path(&relative.to_string_lossy(), strip)?.map(|path| destination.join(path));
        if metadata.is_dir() {
            if let Some(target) = target {
                std::fs::create_dir_all(target).map_err(|err| failure(err.to_string()))?;
            }
            move_members(root, &path, destination, strip)?;
        } else if metadata.is_file() {
            if let Some(target) = target {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|err| failure(err.to_string()))?;
                }
                std::fs::rename(path, target).map_err(|err| failure(err.to_string()))?;
            }
        } else {
            return Err(failure("archive member is not a regular file or directory"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_cross_platform_escape_and_stream_paths() {
        for name in [
            "../outside",
            "root/../../outside",
            "root\\..\\outside",
            "C:\\outside",
            "\\\\server\\share",
            "/outside",
            "root/file:stream",
            "root/.. /outside",
            "root/path./file",
            "root/control\nfile",
        ] {
            assert!(member_path(name, 1).is_err(), "{name}");
        }
        assert_eq!(
            member_path("host/bin/inkscape.com", 1).unwrap(),
            Some(PathBuf::from("bin/inkscape.com"))
        );
    }

    #[test]
    fn refuses_link_members_before_extracting() {
        for line in [
            "Symbolic Link = ../outside",
            "Hard Link = outside",
            "Attributes = A lrwxrwxrwx",
            "Alternate Stream = 1",
        ] {
            assert!(validate_listing(&format!("Path = host/file\n{line}\n")).is_err());
        }
        assert!(validate_listing("Path = host/bin/app.exe\nSize = 4\nAttributes = A\n").is_ok());
        assert!(validate_listing("no member paths").is_err());
    }

    fn zip_fixture(name: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"portable host").unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn extracts_zip_and_strips_the_vendor_folder() {
        let output = tempfile::tempdir().unwrap();
        unpack_zip(&zip_fixture("vendor/bin/app.exe"), output.path(), 1).unwrap();
        assert_eq!(
            std::fs::read(output.path().join("bin/app.exe")).unwrap(),
            b"portable host"
        );
    }

    #[test]
    fn rejects_zip_traversal_before_any_output() {
        let output = tempfile::tempdir().unwrap();
        assert!(unpack_zip(&zip_fixture("vendor/../escaped"), output.path(), 1).is_err());
        assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 0);
    }

    #[test]
    fn extracts_real_7z_when_the_existing_extractor_is_available() {
        if Command::new("7z")
            .arg("i")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            return;
        }
        let fixture = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(fixture.path().join("vendor/bin")).unwrap();
        std::fs::write(fixture.path().join("vendor/bin/app.exe"), b"portable host").unwrap();
        let status = Command::new("7z")
            .current_dir(fixture.path())
            .args(["a", "host.7z", "vendor"])
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        let output = tempfile::tempdir().unwrap();
        unpack_7z(
            &std::fs::read(fixture.path().join("host.7z")).unwrap(),
            output.path(),
            1,
        )
        .unwrap();
        assert_eq!(
            std::fs::read(output.path().join("bin/app.exe")).unwrap(),
            b"portable host"
        );
    }
}
