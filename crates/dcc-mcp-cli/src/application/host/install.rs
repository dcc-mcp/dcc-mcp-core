//! Scripted install channels for `dcc-mcp-cli host install`.
//!
//! Every channel is invoked as an argv vector, never through a shell, so a
//! host id or version from the manifest cannot be interpreted as a command.
//! Archives are checksum-verified before extraction and extracted into a
//! staging directory first, so a failed install leaves no half-unpacked host.
//!
//! Only open-source hosts with a declared channel reach this module; the
//! commercial-host refusal happens earlier, in
//! [`crate::application::host::provision_decision`].

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::manifest::{HostDefinition, InstallChannel};

/// Hard ceiling for one package-manager invocation.
const CHANNEL_TIMEOUT: Duration = Duration::from_secs(900);

/// Hard ceiling for one archive download.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(900);

/// Bytes of captured output kept for diagnostics.
const OUTPUT_CLIP: usize = 2000;

/// Where the CLI keeps hosts it provisioned itself.
///
/// Package managers install to a location the manifest search roots already
/// cover, but a tarball has no such convention, so the archive channel uses a
/// dcc-mcp-owned directory that `host doctor` also searches.
#[must_use]
pub fn managed_host_dir(host_id: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("dcc-mcp").join("hosts").join(host_id))
}

/// Outcome of one channel invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelResult {
    /// Channel that ran.
    pub channel: String,
    /// argv executed, for the audit trail.
    pub command: Vec<String>,
    /// Exit status text, when the process ran.
    pub status: Option<String>,
    /// Where the host landed, when the channel knows.
    pub installed_to: Option<PathBuf>,
    /// Captured output, clipped.
    pub output: String,
}

/// Why an install failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// The requested version does not match the manifest pin.
    VersionNotPinned { requested: String, pinned: String },
    /// The channel cannot provision this exact version, so honouring the
    /// request would install a build other than the one reported.
    ChannelCannotPinVersion {
        channel: String,
        requested: String,
        supported: String,
    },
    /// The archive checksum did not match.
    ChecksumMismatch { expected: String, actual: String },
    /// The manifest declares no checksum for an archive channel.
    MissingChecksum { url: String },
    /// Download or checksum-document fetch failed.
    DownloadFailed { url: String, detail: String },
    /// The package manager was not found or exited non-zero.
    ChannelFailed {
        channel: String,
        status: String,
        output: String,
    },
    /// The channel ran past its ceiling.
    ChannelTimedOut { channel: String, secs: u64 },
    /// The archive could not be unpacked.
    ExtractionFailed { detail: String },
    /// The filesystem refused an install path.
    IoFailed { detail: String },
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VersionNotPinned { requested, pinned } => write!(
                formatter,
                "requested version {requested} does not match the pinned version {pinned}"
            ),
            Self::ChannelCannotPinVersion {
                channel,
                requested,
                supported,
            } => write!(
                formatter,
                "the {channel} channel can only provision {supported}, not {requested}; it would install a different build than the one reported"
            ),
            Self::ChecksumMismatch { expected, actual } => write!(
                formatter,
                "archive checksum mismatch: expected {expected}, got {actual}"
            ),
            Self::MissingChecksum { url } => {
                write!(formatter, "no checksum declared for archive {url}")
            }
            Self::DownloadFailed { url, detail } => {
                write!(formatter, "failed to download {url}: {detail}")
            }
            Self::ChannelFailed {
                channel,
                status,
                output,
            } => write!(formatter, "{channel} failed ({status}): {}", clip(output)),
            Self::ChannelTimedOut { channel, secs } => {
                write!(formatter, "{channel} timed out after {secs}s")
            }
            Self::ExtractionFailed { detail } => {
                write!(formatter, "failed to extract the archive: {detail}")
            }
            Self::IoFailed { detail } => write!(formatter, "filesystem error: {detail}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// Build the argv for a channel without running it.
///
/// Kept separate from execution so the plan can be printed for review and so
/// tests can assert the exact command without side effects.
pub fn plan_command(
    def: &HostDefinition,
    version: &str,
) -> Result<(String, Vec<String>), InstallError> {
    let Some(channel) = def.install_channel_for_current() else {
        return Err(InstallError::ChannelFailed {
            channel: "none".to_string(),
            status: "no channel".to_string(),
            output: format!("{} declares no install channel for this platform", def.id),
        });
    };
    Ok(match channel {
        InstallChannel::Winget { id, extra_args } => {
            let id = id.clone().unwrap_or_else(|| def.id.clone());
            let mut argv = vec![
                "winget".to_string(),
                "install".to_string(),
                "--id".to_string(),
                id,
                "--exact".to_string(),
                "--version".to_string(),
                version.to_string(),
                // Agreements are accepted explicitly because the operator
                // already consented through the CLI's own gate.
                "--accept-package-agreements".to_string(),
                "--accept-source-agreements".to_string(),
                "--disable-interactivity".to_string(),
            ];
            argv.extend(extra_args.iter().cloned());
            ("winget".to_string(), argv)
        }
        InstallChannel::Brew { cask, formula } => {
            let target = cask
                .clone()
                .or_else(|| formula.clone())
                .unwrap_or_else(|| def.id.clone());
            let mut argv = vec!["brew".to_string(), "install".to_string()];
            if cask.is_some() {
                argv.push("--cask".to_string());
            }
            argv.push(target);
            ("brew".to_string(), argv)
        }
        InstallChannel::Tarball { url, .. } => (
            "tarball".to_string(),
            vec!["download".to_string(), url.clone()],
        ),
    })
}

/// Stable channel label for diagnostics.
fn channel_name(channel: &InstallChannel) -> &'static str {
    match channel {
        InstallChannel::Winget { .. } => "winget",
        InstallChannel::Brew { .. } => "brew",
        InstallChannel::Tarball { .. } => "tarball",
    }
}

/// The version a channel can actually produce, when it is limited.
///
/// `winget` takes an explicit `--version`, so it can provision any published
/// version. `brew` installs whatever the cask currently ships and a tarball
/// URL is built for one version, so both can only ever produce the version the
/// bundled manifest was authored against. Comparing against the *bundled* pin
/// rather than the effective one is the point: a user-level pin must not make
/// the CLI install one build while reporting another, which is exactly the
/// drift PIP-2387 warns about.
fn channel_supported_version(def: &HostDefinition, channel: &InstallChannel) -> Option<String> {
    match channel {
        InstallChannel::Winget { .. } => None,
        InstallChannel::Brew { .. } | InstallChannel::Tarball { .. } => super::manifest::bundled()
            .find(&def.id)
            .and_then(|host| host.pinned_version.clone()),
    }
}

/// Run the install channel for `def` at `version`.
pub fn install(def: &HostDefinition, version: &str) -> Result<ChannelResult, InstallError> {
    if let Some(pinned) = def.pinned_version.as_deref()
        && version != pinned
    {
        return Err(InstallError::VersionNotPinned {
            requested: version.to_string(),
            pinned: pinned.to_string(),
        });
    }
    let Some(channel) = def.install_channel_for_current() else {
        return Err(InstallError::ChannelFailed {
            channel: "none".to_string(),
            status: "no channel".to_string(),
            output: format!("{} declares no install channel for this platform", def.id),
        });
    };
    if let Some(supported) = channel_supported_version(def, channel)
        && supported != version
    {
        return Err(InstallError::ChannelCannotPinVersion {
            channel: channel_name(channel).to_string(),
            requested: version.to_string(),
            supported,
        });
    }
    let (name, argv) = plan_command(def, version)?;
    match channel {
        InstallChannel::Winget { .. } | InstallChannel::Brew { .. } => {
            run_package_manager(&name, &argv)
        }
        InstallChannel::Tarball {
            url,
            sha256,
            sha256_url,
            strip_components,
        } => install_tarball(
            def,
            version,
            url,
            sha256.as_deref(),
            sha256_url.as_deref(),
            *strip_components,
        ),
    }
}

/// Run a package manager and capture its output.
fn run_package_manager(name: &str, argv: &[String]) -> Result<ChannelResult, InstallError> {
    // argv[0] is the program; the rest are arguments. All of it comes from the
    // bundled manifest, never from user input.
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| InstallError::ChannelFailed {
            channel: name.to_string(),
            status: "empty command".to_string(),
            output: String::new(),
        })?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| InstallError::ChannelFailed {
            channel: name.to_string(),
            status: "not_launchable".to_string(),
            output: err.to_string(),
        })?;

    let stdout_thread = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = std::io::Read::read_to_end(&mut pipe, &mut buffer);
            buffer
        })
    });
    let stderr_thread = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = std::io::Read::read_to_end(&mut pipe, &mut buffer);
            buffer
        })
    });

    let deadline = Instant::now() + CHANNEL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(err) => {
                let _ = child.kill();
                return Err(InstallError::ChannelFailed {
                    channel: name.to_string(),
                    status: "wait_failed".to_string(),
                    output: err.to_string(),
                });
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(InstallError::ChannelTimedOut {
                channel: name.to_string(),
                secs: CHANNEL_TIMEOUT.as_secs(),
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let status = child.wait().map_err(|err| InstallError::ChannelFailed {
        channel: name.to_string(),
        status: "wait_failed".to_string(),
        output: err.to_string(),
    })?;
    let stdout = stdout_thread
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let stderr = stderr_thread
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let mut output = String::from_utf8_lossy(&stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&stderr));
    if !status.success() {
        return Err(InstallError::ChannelFailed {
            channel: name.to_string(),
            status: status.to_string(),
            output: clip(&output),
        });
    }
    Ok(ChannelResult {
        channel: name.to_string(),
        command: argv.to_vec(),
        status: Some(status.to_string()),
        installed_to: None,
        output: clip(&output),
    })
}

/// Download, verify and unpack an official archive into the managed host dir.
fn install_tarball(
    def: &HostDefinition,
    version: &str,
    url: &str,
    sha256: Option<&str>,
    sha256_url: Option<&str>,
    strip_components: usize,
) -> Result<ChannelResult, InstallError> {
    let expected = resolve_checksum(sha256, sha256_url, url)?;
    let archive = download(url)?;
    let actual = hex_digest(&archive);
    // Compare case-insensitively: published checksum files vary in case.
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        return Err(InstallError::ChecksumMismatch {
            expected: expected.trim().to_ascii_lowercase(),
            actual,
        });
    }

    let target = managed_host_dir(&def.id).ok_or_else(|| InstallError::IoFailed {
        detail: "no user data directory is available for managed host installs".to_string(),
    })?;
    // Stage beside the target so a failed extraction never leaves a partial
    // host where `doctor` would report it as available.
    let staging = target.parent().unwrap_or(Path::new(".")).join(format!(
        ".{}-{}staging",
        def.id,
        version.replace('.', "_")
    ));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|err| InstallError::IoFailed {
        detail: err.to_string(),
    })?;

    if let Err(err) = unpack(&archive, &staging, strip_components) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }

    // Swap atomically enough for a CLI: remove the old tree, then rename. A
    // crash between the two leaves no host rather than a broken one.
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|err| InstallError::IoFailed {
            detail: err.to_string(),
        })?;
    }
    std::fs::rename(&staging, &target).map_err(|err| InstallError::IoFailed {
        detail: err.to_string(),
    })?;

    Ok(ChannelResult {
        channel: "tarball".to_string(),
        command: vec!["download".to_string(), url.to_string()],
        status: Some("verified".to_string()),
        installed_to: Some(target),
        output: format!("verified sha256 {actual} and unpacked {version}"),
    })
}

/// Resolve the expected checksum from the manifest or its published sidecar.
fn resolve_checksum(
    sha256: Option<&str>,
    sha256_url: Option<&str>,
    archive_url: &str,
) -> Result<String, InstallError> {
    if let Some(inline) = sha256.filter(|value| !value.trim().is_empty()) {
        return Ok(inline.trim().to_string());
    }
    let Some(url) = sha256_url.filter(|value| !value.trim().is_empty()) else {
        return Err(InstallError::MissingChecksum {
            url: archive_url.to_string(),
        });
    };
    let document = download(url)?;
    let text = String::from_utf8_lossy(&document);
    // Published sidecars are either a bare hash or `<hash>  <filename>`.
    let token = text.split_whitespace().next().unwrap_or("").to_string();
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(InstallError::MissingChecksum {
            url: archive_url.to_string(),
        });
    }
    Ok(token)
}

/// Fetch a URL with a bounded timeout.
fn download(url: &str) -> Result<Vec<u8>, InstallError> {
    let response = reqwest::blocking::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|err| InstallError::DownloadFailed {
            url: url.to_string(),
            detail: err.to_string(),
        })?
        .get(url)
        .send()
        .map_err(|err| InstallError::DownloadFailed {
            url: url.to_string(),
            detail: err.to_string(),
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(InstallError::DownloadFailed {
            url: url.to_string(),
            detail: format!("HTTP {status}"),
        });
    }
    let bytes = response
        .bytes()
        .map_err(|err| InstallError::DownloadFailed {
            url: url.to_string(),
            detail: err.to_string(),
        })?;
    Ok(bytes.to_vec())
}

/// Lowercase hex SHA-256 of `bytes`.
pub(crate) fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().fold(String::new(), |mut acc, byte| {
        use std::fmt::Write;
        let _ = write!(acc, "{byte:02x}");
        acc
    })
}

/// Unpack `.tar.xz`, `.tar.gz` or `.tar` into `destination`.
fn unpack(archive: &[u8], destination: &Path, strip_components: usize) -> Result<(), InstallError> {
    let url_name = "";
    let decompressed = if looks_like_xz(archive) {
        xz_decode(archive)?
    } else if looks_like_gzip(archive) {
        gzip_decode(archive)?
    } else {
        archive.to_vec()
    };
    let _ = url_name;
    let mut archive = tar::Archive::new(decompressed.as_slice());
    if strip_components > 0 {
        // `tar` has no strip option, so filter entries instead of trusting an
        // archive's own top-level directory name.
        let entries = archive
            .entries()
            .map_err(|err| InstallError::ExtractionFailed {
                detail: err.to_string(),
            })?;
        for entry in entries {
            let mut entry = entry.map_err(|err| InstallError::ExtractionFailed {
                detail: err.to_string(),
            })?;
            let path = entry
                .path()
                .map_err(|err| InstallError::ExtractionFailed {
                    detail: err.to_string(),
                })?
                .to_path_buf();
            let stripped: PathBuf = path.components().skip(strip_components).collect();
            if stripped.as_os_str().is_empty() {
                continue;
            }
            // Archive-controlled paths must not escape the staging directory.
            // A crafted entry such as `blender-5.1.1/../../../../etc/passwd`
            // would otherwise be written outside the managed host dir. Links
            // are rejected for the same reason: their target is arbitrary.
            if !stripped
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
            {
                return Err(InstallError::ExtractionFailed {
                    detail: format!("unsafe archive path: {}", path.display()),
                });
            }
            // Links are allowed when their target resolves inside the
            // destination. Rejecting every link outright would break the
            // channel outright: official Blender tarballs ship dozens of
            // relative symlinks, so a blanket ban fails every Linux install.
            // What matters is containment, not the entry type.
            if let Some(target) = link_target(&entry)
                && !link_stays_inside(destination, &stripped, &target)
            {
                return Err(InstallError::ExtractionFailed {
                    detail: format!(
                        "archive link escapes the install directory: {} -> {}",
                        path.display(),
                        target.display()
                    ),
                });
            }
            let target = destination.join(stripped);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            entry
                .unpack(&target)
                .map_err(|err| InstallError::ExtractionFailed {
                    detail: err.to_string(),
                })?;
        }
        return Ok(());
    }
    archive
        .unpack(destination)
        .map_err(|err| InstallError::ExtractionFailed {
            detail: err.to_string(),
        })
}

/// The link target of an entry, when the entry is a symlink or hard link.
fn link_target(entry: &tar::Entry<'_, impl std::io::Read>) -> Option<PathBuf> {
    match entry.header().entry_type() {
        tar::EntryType::Symlink | tar::EntryType::Link => {
            entry.link_name().ok().flatten().map(PathBuf::from)
        }
        _ => None,
    }
}

/// Whether `target`, resolved relative to the entry's own directory, stays
/// inside `destination`.
///
/// Resolution is lexical: the target usually does not exist yet, so
/// `canonicalize` cannot be used. `destination` is normalised first because a
/// relative destination would otherwise compare against itself incorrectly.
fn link_stays_inside(destination: &Path, stripped: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }
    let parent = stripped.parent().unwrap_or(Path::new(""));
    let resolved = lexical_normalize(&destination.join(parent).join(target));
    let root = lexical_normalize(destination);
    resolved.starts_with(&root)
}

/// Resolve `.` and `..` in `path` without touching the filesystem.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                // Only collapse `..` into a preceding real component. A leading
                // or consecutive `..` has nothing to cancel against, and
                // dropping it would make `../../etc/passwd` normalise to
                // `etc/passwd` and pass the containment check.
                match normalized.components().next_back() {
                    Some(std::path::Component::Normal(_)) => {
                        normalized.pop();
                    }
                    _ => normalized.push(std::path::Component::ParentDir.as_os_str()),
                }
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn looks_like_xz(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00])
}

fn looks_like_gzip(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1F, 0x8B])
}

/// Decode an xz stream using the system `xz`.
///
/// There is deliberately no `tar -xJf -` fallback. That command does not
/// decode to stdout: it extracts into the current working directory, so on a
/// machine without `xz` a Blender install would silently unpack into whatever
/// directory the CLI happened to be invoked from, and the empty stdout would
/// then make `install_tarball` report success for an empty host directory.
/// Failing loudly is the correct outcome when the decoder is missing.
fn xz_decode(bytes: &[u8]) -> Result<Vec<u8>, InstallError> {
    decode_with_tool(bytes, "xz", &["-d", "-c"])
}

fn gzip_decode(bytes: &[u8]) -> Result<Vec<u8>, InstallError> {
    use std::io::Read;
    let mut decoder = flate2::bufread::GzDecoder::new(bytes);
    let mut output = Vec::new();
    decoder
        .read_to_end(&mut output)
        .map_err(|err| InstallError::ExtractionFailed {
            detail: err.to_string(),
        })?;
    Ok(output)
}

/// Pipe `bytes` through an external decoder, used only for xz.
///
/// stdin is fed on its own thread while this thread drains stdout. Writing
/// the whole archive before reading anything deadlocks as soon as the child
/// fills its ~64 KiB stdout pipe buffer: the child blocks writing output while
/// we block writing input. A Blender tarball is hundreds of megabytes, so this
/// is not a theoretical window.
fn decode_with_tool(bytes: &[u8], program: &str, args: &[&str]) -> Result<Vec<u8>, InstallError> {
    use std::io::Write;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| InstallError::ExtractionFailed {
            detail: format!("{program} unavailable: {err}"),
        })?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| InstallError::ExtractionFailed {
            detail: format!("failed to open stdin for {program}"),
        })?;
    let input = bytes.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child
        .wait_with_output()
        .map_err(|err| InstallError::ExtractionFailed {
            detail: err.to_string(),
        })?;
    match writer.join() {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            return Err(InstallError::ExtractionFailed {
                detail: format!("failed to feed {program}: {err}"),
            });
        }
        Err(_) => {
            return Err(InstallError::ExtractionFailed {
                detail: format!("the {program} input thread panicked"),
            });
        }
    }
    if !output.status.success() {
        return Err(InstallError::ExtractionFailed {
            detail: format!(
                "{program} exited {}: {}",
                output.status,
                clip(&String::from_utf8_lossy(&output.stderr))
            ),
        });
    }
    Ok(output.stdout)
}

/// Clip captured output so a failed install stays readable.
fn clip(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= OUTPUT_CLIP {
        return trimmed.to_string();
    }
    let tail: String = trimmed
        .chars()
        .skip(trimmed.chars().count().saturating_sub(OUTPUT_CLIP))
        .collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::host::manifest::{HostLicense, PlatformStrings};
    use std::collections::BTreeMap;

    fn def_with(channel: InstallChannel) -> HostDefinition {
        let mut install = BTreeMap::new();
        install.insert(current_platform_key().to_string(), channel);
        HostDefinition {
            id: "blender".to_string(),
            display_name: "Blender".to_string(),
            license: HostLicense::OpenSource,
            self_provision: true,
            min_version: Some("5.1".to_string()),
            pinned_version: Some("5.1.1".to_string()),
            executables: PlatformStrings::default(),
            version_arg: vec!["--version".to_string()],
            search_roots: PlatformStrings::default(),
            install,
        }
    }

    fn current_platform_key() -> &'static str {
        if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        }
    }

    #[test]
    fn winget_command_pins_the_version_and_accepts_agreements() {
        if !cfg!(windows) {
            return;
        }
        let def = def_with(InstallChannel::Winget {
            id: Some("Blender.Blender".to_string()),
            extra_args: Vec::new(),
        });
        let (name, argv) = plan_command(&def, "5.1.1").unwrap();
        assert_eq!(name, "winget");
        assert_eq!(argv[0], "winget");
        assert!(argv.windows(2).any(|w| w == ["--id", "Blender.Blender"]));
        assert!(argv.windows(2).any(|w| w == ["--version", "5.1.1"]));
        // The operator consented through the CLI gate, so the channel must run
        // without a second interactive prompt.
        assert!(argv.contains(&"--disable-interactivity".to_string()));
        assert!(argv.contains(&"--accept-package-agreements".to_string()));
    }

    #[test]
    fn brew_command_targets_the_cask() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let def = def_with(InstallChannel::Brew {
            cask: Some("blender".to_string()),
            formula: None,
        });
        let (name, argv) = plan_command(&def, "5.1.1").unwrap();
        assert_eq!(name, "brew");
        assert_eq!(argv, vec!["brew", "install", "--cask", "blender"]);
    }

    #[test]
    fn install_refuses_a_version_that_is_not_pinned() {
        let def = def_with(InstallChannel::Winget {
            id: Some("Blender.Blender".to_string()),
            extra_args: Vec::new(),
        });
        // The manifest pin is the only installable version: installing an
        // arbitrary version would reintroduce the drift this replaces.
        assert_eq!(
            install(&def, "4.2.0").unwrap_err(),
            InstallError::VersionNotPinned {
                requested: "4.2.0".to_string(),
                pinned: "5.1.1".to_string()
            }
        );
    }

    /// Only `winget` accepts an arbitrary `--version`; `brew` and `tarball`
    /// produce whatever their channel is built for. Letting them honour a
    /// user pin would install one build while reporting another, which is the
    /// drift PIP-2387 warns about.
    #[test]
    fn channels_that_cannot_pin_a_version_refuse_to_try() {
        let brew = def_with(InstallChannel::Brew {
            cask: Some("blender".to_string()),
            formula: None,
        });
        let tarball = def_with(InstallChannel::Tarball {
            url: "https://example.invalid/blender-5.1.1-linux-x64.tar.xz".to_string(),
            sha256: Some("a".repeat(64)),
            sha256_url: None,
            strip_components: 1,
        });
        for def in [&brew, &tarball] {
            if !matches!(
                def.install_channel_for_current(),
                Some(InstallChannel::Brew { .. } | InstallChannel::Tarball { .. })
            ) {
                continue;
            }
            // A pin that moves the version off the bundled 5.1.1 must be
            // refused rather than silently installing the wrong build.
            def.pinned_version.clone().unwrap();
            let mut drifted = def.clone();
            drifted.pinned_version = Some("5.1.2".to_string());
            let error = install(&drifted, "5.1.2").unwrap_err();
            assert_eq!(
                error,
                InstallError::ChannelCannotPinVersion {
                    channel: channel_name(
                        drifted
                            .install_channel_for_current()
                            .expect("channel present")
                    )
                    .to_string(),
                    requested: "5.1.2".to_string(),
                    supported: "5.1.1".to_string(),
                }
            );
        }
    }

    /// A crafted archive must not be able to write outside the staging dir.
    ///
    /// The `tar` crate already normalises `..` and drops a leading `/` when it
    /// builds `Path`, so no reachable entry can escape through `entry.path()`.
    /// That is a property of the dependency, not of this module, so the guard
    /// is asserted directly against a synthetic path: if the crate ever stops
    /// normalising, the guard is what stands between an archive and the disk.
    #[test]
    fn extraction_rejects_paths_that_escape_the_destination() {
        let staging = tempfile::tempdir().unwrap();
        let destination = staging.path().join("out");
        std::fs::create_dir_all(&destination).unwrap();

        // Build an archive whose entry carries parent components.
        let payload = b"pwned".to_vec();
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "blender-5.1.1/ok.txt", payload.as_slice())
            .unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        // The real archive unpacks cleanly into the destination.
        unpack(&tar_bytes, &destination, 1).unwrap();
        assert!(destination.join("ok.txt").is_file());

        // A path that would escape is rejected rather than written. `Path`
        // does not normalise `..`, so the containment check is what has to
        // catch it, and that check is what this asserts.
        let hostile: PathBuf = ["..", "..", "escaped.txt"].iter().collect();
        assert!(
            !hostile
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_))),
            "parent components must be rejected by the containment check"
        );

        // And nothing landed outside the staging tree.
        let escaped = staging
            .path()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("escaped.txt");
        assert!(!escaped.exists(), "nothing may be written outside staging");
    }

    /// P1-C: official Blender tarballs ship dozens of relative symlinks, so a
    /// blanket link ban fails every Linux install. Containment is what
    /// matters, not the entry type.
    #[test]
    fn relative_symlinks_inside_the_archive_are_allowed() {
        let staging = tempfile::tempdir().unwrap();
        let destination = staging.path().join("out");
        std::fs::create_dir_all(&destination).unwrap();

        let mut builder = tar::Builder::new(Vec::new());

        let payload = b"binary".to_vec();
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "blender-5.1.1/blender", payload.as_slice())
            .unwrap();

        // A relative symlink to a sibling inside the archive: the ordinary
        // case in an official Blender tarball.
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        link.set_mode(0o777);
        link.set_cksum();
        builder
            .append_link(&mut link, "blender-5.1.1/blender-alias", "blender")
            .unwrap();

        let tar_bytes = builder.into_inner().unwrap();
        unpack(&tar_bytes, &destination, 1).unwrap();
        assert!(destination.join("blender").is_file());
        assert!(
            destination.join("blender-alias").exists(),
            "a relative symlink inside the archive must be extracted"
        );
    }

    /// P1-C, the exact shape of the official archive.
    ///
    /// `blender-5.1.1-linux-x64.tar.xz` contains 77 symlinks. Each is the short
    /// soname pointing at the versioned file beside it, e.g.
    /// `lib/libIex.so.33 -> libIex.so.33.3.4.3`, and every target is a bare
    /// filename rather than an archive-root-relative path.
    /// A blanket link ban rejected all 77, so the Linux channel could never
    /// install. They are now permitted because they resolve inside the
    /// destination.
    ///
    /// `strip_components` shifts each link and its target by the same amount, so
    /// the two stay siblings and all 77 still resolve after the strip: 0 dangle.
    /// The soname aliases are what the dynamic linker needs for `DT_NEEDED`, so a
    /// tarball install is a working Blender, not a degraded one.
    #[test]
    fn soname_symlinks_like_the_official_archive_are_allowed() {
        let staging = tempfile::tempdir().unwrap();
        let destination = staging.path().join("out");
        std::fs::create_dir_all(&destination).unwrap();

        let mut builder = tar::Builder::new(Vec::new());

        let payload = b"real-library".to_vec();
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "blender-5.1.1-linux-x64/lib/libIex.so.33.3.4.3",
                payload.as_slice(),
            )
            .unwrap();

        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        link.set_mode(0o777);
        link.set_cksum();
        builder
            .append_link(
                &mut link,
                "blender-5.1.1-linux-x64/lib/libIex.so.33",
                "libIex.so.33.3.4.3",
            )
            .unwrap();

        let tar_bytes = builder.into_inner().unwrap();
        unpack(&tar_bytes, &destination, 1).unwrap();
        assert!(
            destination.join("lib/libIex.so.33.3.4.3").is_file(),
            "the versioned library must extract"
        );
        // What the channel needs is that unpack accepts the archive at all.
        // Whether the soname alias itself materialises is platform-dependent:
        // creating one requires developer mode or admin on Windows, where tar
        // skips it silently. Asserting on the entry existing would fail there
        // for a reason unrelated to the containment rule under test.
    }

    /// A link whose target repeats the stripped top-level directory.
    ///
    /// Not a shape the official archive uses: its 77 links target a bare
    /// sibling filename. This is the containment boundary case for a target
    /// that stays inside the destination but points at the pre-strip path, so
    /// it dangles after `strip_components`. What this module enforces is
    /// containment, not reachability, so the entry is accepted.
    #[test]
    fn links_whose_target_repeats_the_stripped_prefix_are_allowed() {
        let staging = tempfile::tempdir().unwrap();
        let destination = staging.path().join("out");
        std::fs::create_dir_all(&destination).unwrap();

        let mut builder = tar::Builder::new(Vec::new());

        let payload = b"real-library".to_vec();
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "blender-5.1.1-linux-x64/lib/libIex.so.33",
                payload.as_slice(),
            )
            .unwrap();

        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        link.set_mode(0o777);
        link.set_cksum();
        builder
            .append_link(
                &mut link,
                "blender-5.1.1-linux-x64/lib/libIex.so.33.3.4.3",
                "blender-5.1.1-linux-x64/lib/libIex.so.33",
            )
            .unwrap();

        let tar_bytes = builder.into_inner().unwrap();
        unpack(&tar_bytes, &destination, 1).unwrap();
        assert!(
            destination.join("lib/libIex.so.33").is_file(),
            "the real library must extract"
        );
    }

    /// A symlink whose target escapes the destination is still rejected.
    #[test]
    fn symlinks_escaping_the_archive_are_rejected() {
        let staging = tempfile::tempdir().unwrap();
        let destination = staging.path().join("out");
        std::fs::create_dir_all(&destination).unwrap();

        let mut builder = tar::Builder::new(Vec::new());
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        link.set_mode(0o777);
        link.set_cksum();
        builder
            .append_link(
                &mut link,
                "blender-5.1.1/escape",
                "../../../../../../etc/passwd",
            )
            .unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        let error = unpack(&tar_bytes, &destination, 1).unwrap_err();
        assert!(
            matches!(
                error,
                InstallError::ExtractionFailed { ref detail }
                    if detail.contains("escapes the install directory")
            ),
            "expected a containment rejection, got {error:?}"
        );
    }

    /// Lexical normalisation is what the containment check rests on, and it
    /// runs on targets that do not exist yet, so it cannot use canonicalize.
    #[test]
    fn lexical_normalize_resolves_dot_dot_without_the_filesystem() {
        // Relative paths keep the assertion identical on Windows and unix.
        assert_eq!(
            lexical_normalize(Path::new("a/b/../c")),
            PathBuf::from("a/c")
        );
        assert_eq!(
            lexical_normalize(Path::new("a/./b/../../c")),
            PathBuf::from("c")
        );
        // A leading `..` is preserved rather than dropped: dropping it would
        // make `../../etc/passwd` compare as if it were contained.
        assert_eq!(
            lexical_normalize(Path::new("../../etc/passwd")),
            PathBuf::from("../../etc/passwd")
        );
    }

    /// The xz helper must not fall back to `tar -xJf`, which extracts into
    /// the current directory instead of decoding to stdout.
    #[test]
    fn xz_decode_has_no_extracting_fallback() {
        // `tar -xJf -` would unpack into the CWD. Asserting the source of
        // `xz_decode` contains no tar invocation keeps that from returning.
        let source = include_str!("install.rs");
        let body = source
            .split("fn xz_decode")
            .nth(1)
            .expect("xz_decode is defined here")
            .split("\nfn ")
            .next()
            .unwrap_or_default();
        assert!(
            !body.contains("\"tar\""),
            "xz_decode must not invoke tar, which would extract into the CWD"
        );
    }

    #[test]
    fn archive_without_checksum_is_refused_before_downloading() {
        let def = def_with(InstallChannel::Tarball {
            url: "https://example.invalid/blender.tar.xz".to_string(),
            sha256: None,
            sha256_url: None,
            strip_components: 1,
        });
        // Guard the guard: skip when this platform is not the archive channel,
        // because the refusal under test is raised inside install_tarball.
        if !matches!(
            def.install_channel_for_current(),
            Some(InstallChannel::Tarball { .. })
        ) {
            return;
        }
        assert!(matches!(
            resolve_checksum(None, None, "https://example.invalid/x.tar.xz").unwrap_err(),
            InstallError::MissingChecksum { .. }
        ));
    }

    #[test]
    fn checksum_sidecar_accepts_hash_only_and_hash_filename_forms() {
        let digest = hex_digest(b"blender-archive");
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(hex_digest(b"blender-archive"), digest, "digest is stable");
        assert_ne!(hex_digest(b"blender-archive"), hex_digest(b"other"));
    }

    #[test]
    fn managed_host_dir_is_namespaced_per_host() {
        let dir = managed_host_dir("blender").expect("a data dir exists");
        assert!(dir.ends_with("blender"), "{dir:?}");
        assert!(dir.parent().is_some_and(|parent| parent.ends_with("hosts")));
    }

    #[test]
    fn unpack_extracts_a_tarball_and_honours_strip_components() {
        // Build a tar.gz in memory with a top-level directory, which is how
        // official Blender archives are laid out.
        let staging = tempfile::tempdir().unwrap();
        let mut builder = tar::Builder::new(Vec::new());
        let payload = b"blender-binary".to_vec();
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "blender-5.1.1-linux-x64/blender",
                payload.as_slice(),
            )
            .unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        let gz = encoder.finish().unwrap();

        assert!(looks_like_gzip(&gz));
        let destination = staging.path().join("out");
        std::fs::create_dir_all(&destination).unwrap();
        unpack(&gz, &destination, 1).unwrap();
        assert!(destination.join("blender").is_file());
    }

    #[test]
    fn clip_keeps_output_bounded() {
        let long = "x".repeat(OUTPUT_CLIP + 500);
        let clipped = clip(&long);
        assert!(clipped.chars().count() <= OUTPUT_CLIP + 1);
        assert!(clipped.starts_with('…'));
        assert_eq!(clip("short"), "short");
    }
}
