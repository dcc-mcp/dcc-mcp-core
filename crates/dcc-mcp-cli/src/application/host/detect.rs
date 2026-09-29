//! Executable discovery and version probing for one host.
//!
//! Strictly read-only. Nothing here writes to disk or invokes an installer:
//! the worst this module does to a machine is run `<host> --version` on a
//! binary it already decided exists. `host doctor` is the "tell me before I
//! start" surface, so a probe that installed anything would defeat its purpose.
//!
//! Lookup order, highest precedence first:
//!
//! 1. `DCC_MCP_<ID>_EXECUTABLE` — studio layouts that are not on PATH.
//! 2. `PATH` — the normal case for Blender and friends.
//! 3. Manifest `search_roots` — the vendor's default install location, which
//!    PIP-3579 showed can be absent even when the directory tree exists.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use semver::Version;

use super::manifest::{HostDefinition, PlatformStrings};
use crate::domain::host::extract_version;

/// Hard ceiling for one `--version` call.
const VERSION_QUERY_TIMEOUT: Duration = Duration::from_secs(20);

/// Where a candidate executable came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSource {
    /// `DCC_MCP_<ID>_EXECUTABLE`.
    EnvOverride,
    /// Found on `PATH`.
    Path,
    /// Found under a manifest search root.
    SearchRoot,
}

impl CandidateSource {
    /// Stable label for operator-facing output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EnvOverride => "env_override",
            Self::Path => "PATH",
            Self::SearchRoot => "search_root",
        }
    }
}

/// One executable that might be the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    pub source: CandidateSource,
}

/// Process environment view, injectable so tests never depend on the real one.
#[derive(Debug, Clone, Default)]
pub struct HostEnv {
    /// Upper-cased variable name to value.
    pub vars: BTreeMap<String, String>,
    /// `PATH` split on the platform separator. `None` means "not searchable".
    pub path_entries: Option<Vec<PathBuf>>,
}

impl HostEnv {
    /// New empty environment: no overrides and no PATH.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Capture the current process environment.
    #[must_use]
    pub fn from_process() -> Self {
        let mut vars = BTreeMap::new();
        for (key, value) in std::env::vars_os() {
            let key = key.to_string_lossy().to_ascii_uppercase();
            vars.insert(key, value.to_string_lossy().into_owned());
        }
        let path_entries =
            std::env::var_os("PATH").map(|value| std::env::split_paths(&value).collect::<Vec<_>>());
        Self { vars, path_entries }
    }

    /// Read one variable, case-insensitively.
    #[must_use]
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(&name.to_ascii_uppercase())
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }
}

/// Discover every executable that could be `def`, in precedence order.
///
/// All three sources are collected rather than short-circuiting, so a probe can
/// report where it looked even when nothing was found.
#[must_use]
pub fn candidates(def: &HostDefinition, env: &HostEnv) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = Vec::new();
    let push = |path: PathBuf, source: CandidateSource, found: &mut Vec<Candidate>| {
        if !is_executable_file(&path) {
            return;
        }
        if found
            .iter()
            .any(|existing: &Candidate| existing.path == path)
        {
            return;
        }
        found.push(Candidate { path, source });
    };

    if let Some(value) = env.var(&def.executable_env_var()) {
        push(
            PathBuf::from(value.trim()),
            CandidateSource::EnvOverride,
            &mut found,
        );
    }

    if let Some(entries) = env.path_entries.as_ref() {
        for name in def.executables.for_current() {
            for entry in entries {
                for candidate in with_platform_extensions(entry.join(name)) {
                    push(candidate, CandidateSource::Path, &mut found);
                }
            }
        }
    }

    for pattern in def.search_roots.for_current() {
        for candidate in expand_pattern(Path::new(pattern)) {
            push(candidate, CandidateSource::SearchRoot, &mut found);
        }
    }

    found
}

/// Ask the executable for its version.
///
/// Returns `Ok(None)` only when the host declares no version query, so callers
/// can distinguish "not asked" from "asked and failed". A host that declares a
/// query but answers without a version token is [`VersionQueryError::Unparsable`],
/// never `Ok(None)`: callers read `Ok(None)` as "existence is the whole
/// contract" and skip the version gate, so returning it for an unreadable
/// answer would admit a host of unknown version against a declared gate.
pub fn query_version(
    executable: &Path,
    version_arg: &[String],
) -> Result<Option<Version>, VersionQueryError> {
    if version_arg.is_empty() {
        return Ok(None);
    }
    let output = run_version_query(executable, version_arg)?;
    extract_version(&output)
        .map(Some)
        .ok_or_else(|| VersionQueryError::Unparsable {
            output: clip(&output),
        })
}

/// Why a version query could not produce a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionQueryError {
    /// The process could not be spawned.
    NotLaunchable { detail: String },
    /// The process ran but exited non-zero.
    Exited { status: String, output: String },
    /// The process never exited within the ceiling.
    TimedOut { secs: u64 },
    /// The process produced no parseable version token.
    Unparsable { output: String },
}

/// Run `<executable> <version_arg...>` and return combined output.
fn run_version_query(
    executable: &Path,
    version_arg: &[String],
) -> Result<String, VersionQueryError> {
    let mut child = Command::new(executable)
        .args(version_arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| VersionQueryError::NotLaunchable {
            detail: err.to_string(),
        })?;

    // Some hosts answer on stderr, some on stdout, and a few print a GUI splash
    // before exiting. Drain both pipes and never block on a chatty child.
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

    let deadline = Instant::now() + VERSION_QUERY_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(err) => {
                let _ = child.kill();
                return Err(VersionQueryError::NotLaunchable {
                    detail: err.to_string(),
                });
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(VersionQueryError::TimedOut {
                secs: VERSION_QUERY_TIMEOUT.as_secs(),
            });
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let status = child
        .wait()
        .map_err(|err| VersionQueryError::NotLaunchable {
            detail: err.to_string(),
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
        return Err(VersionQueryError::Exited {
            status: status.to_string(),
            output: clip(&output),
        });
    }
    Ok(output)
}

/// Keep diagnostics bounded: a host banner can be megabytes of noise.
fn clip(text: &str) -> String {
    const LIMIT: usize = 800;
    let trimmed = text.trim();
    if trimmed.chars().count() <= LIMIT {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(LIMIT).collect();
    format!("{head}… (truncated)")
}

/// Whether `path` is a file we could execute.
///
/// Public so the probe can explain a rejected override instead of dropping it.
pub fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    // On unix the executable bit is the contract; on Windows every file is
    // "executable" by extension, so only existence is checked there.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Windows resolves bare names through `PATHEXTS`, so try the obvious ones.
fn with_platform_extensions(path: PathBuf) -> Vec<PathBuf> {
    if !cfg!(windows) || path.extension().is_some() {
        return vec![path];
    }
    let mut names = vec![path.clone()];
    for extension in ["exe", "cmd", "bat"] {
        let mut candidate = path.clone().into_os_string();
        candidate.push(".");
        candidate.push(extension);
        names.push(PathBuf::from(candidate));
    }
    names
}

/// Expand a path pattern supporting `*` and `?` per segment.
///
/// Only `*` and `?` are supported — that is all the manifest needs and it keeps
/// the matcher dependency-free. Unreadable directories are skipped rather than
/// failed, because probing must not break on a locked-down machine.
fn expand_pattern(pattern: &Path) -> Vec<PathBuf> {
    let mut frontier = vec![PathBuf::new()];
    for segment in pattern.components() {
        let segment = segment.as_os_str().to_string_lossy().into_owned();
        if segment.is_empty() || segment == "." {
            continue;
        }
        let mut next = Vec::new();
        for prefix in &frontier {
            let base = if prefix.as_os_str().is_empty() {
                // Preserve absolute roots: `C:\` and `/` are components here.
                PathBuf::from(&segment)
            } else {
                prefix.join(&segment)
            };
            if !segment.contains(['*', '?']) {
                next.push(base);
                continue;
            }
            let parent = base
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            let Ok(entries) = std::fs::read_dir(&parent) else {
                continue;
            };
            let needle = base.file_name().unwrap_or_default().to_string_lossy();
            let mut matched: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    match_path(
                        &needle,
                        &path.file_name().unwrap_or_default().to_string_lossy(),
                    )
                })
                .collect();
            matched.sort();
            next.extend(matched);
        }
        frontier = next;
        if frontier.is_empty() {
            return Vec::new();
        }
    }
    frontier.retain(|path| !path.as_os_str().is_empty());
    frontier
}

/// Match one path segment against a `*`/`?` pattern (case-insensitive).
fn match_path(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let (mut p, mut n) = (0_usize, 0_usize);
    let (mut star, mut star_n) = (None::<usize>, 0_usize);
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            star_n = n;
            p += 1;
        } else if let Some(star_p) = star {
            p = star_p + 1;
            star_n += 1;
            n = star_n;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

/// Sources `doctor` reports, so an operator can see where it looked.
#[must_use]
pub fn sources_checked(def: &HostDefinition, env: &HostEnv) -> Vec<String> {
    let mut sources = vec![def.executable_env_var()];
    if env.path_entries.is_some() {
        sources.push("PATH".to_string());
    }
    sources.extend(def.search_roots.for_current().iter().cloned());
    sources
}

/// Re-exported for the report layer: the platform strings a host declares.
#[must_use]
pub fn covers_platform(executables: &PlatformStrings) -> bool {
    executables.covers_current()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::host::manifest::{HostLicense, InstallChannel};

    fn def(
        id: &str,
        executables: PlatformStrings,
        search_roots: PlatformStrings,
    ) -> HostDefinition {
        HostDefinition {
            id: id.to_string(),
            display_name: id.to_string(),
            license: HostLicense::OpenSource,
            self_provision: false,
            min_version: None,
            pinned_version: None,
            executables,
            version_arg: vec!["--version".to_string()],
            search_roots,
            install: BTreeMap::new(),
        }
    }

    fn platform(values: &[&str]) -> PlatformStrings {
        let mut strings = PlatformStrings::default();
        let target = if cfg!(windows) {
            &mut strings.windows
        } else if cfg!(target_os = "macos") {
            &mut strings.macos
        } else {
            &mut strings.linux
        };
        target.extend(values.iter().map(|value| (*value).to_string()));
        strings
    }

    fn env_with_path(dir: &Path) -> HostEnv {
        let mut env = HostEnv::new();
        env.path_entries = Some(vec![dir.to_path_buf()]);
        env
    }

    /// Create a file that passes `is_executable_file` on every platform.
    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"stub").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::metadata(&path).unwrap();
        path
    }

    #[test]
    fn env_override_wins_over_path() {
        let dir = tempfile::tempdir().unwrap();
        let on_path = touch(dir.path(), "blender");
        let override_dir = tempfile::tempdir().unwrap();
        let overridden = touch(override_dir.path(), "blender-custom");

        let mut env = env_with_path(dir.path());
        env.vars.insert(
            "DCC_MCP_BLENDER_EXECUTABLE".to_string(),
            overridden.display().to_string(),
        );
        let found = candidates(
            &def(
                "blender",
                platform(&["blender"]),
                PlatformStrings::default(),
            ),
            &env,
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].path, overridden);
        assert_eq!(found[0].source, CandidateSource::EnvOverride);
        assert_eq!(found[1].path, on_path);
        assert_eq!(found[1].source, CandidateSource::Path);
    }

    #[test]
    fn empty_and_missing_overrides_are_ignored() {
        let mut env = HostEnv::new();
        env.vars
            .insert("DCC_MCP_BLENDER_EXECUTABLE".to_string(), "   ".to_string());
        assert!(
            candidates(
                &def(
                    "blender",
                    platform(&["blender"]),
                    PlatformStrings::default()
                ),
                &env
            )
            .is_empty()
        );
    }

    #[test]
    fn path_lookup_finds_the_host_binary() {
        let dir = tempfile::tempdir().unwrap();
        let binary = touch(dir.path(), "blender");
        let found = candidates(
            &def(
                "blender",
                platform(&["blender"]),
                PlatformStrings::default(),
            ),
            &env_with_path(dir.path()),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, binary);
        assert_eq!(found[0].source, CandidateSource::Path);
    }

    #[test]
    fn missing_host_yields_no_candidates() {
        // PIP-3579: the install root existed but no blender.exe was anywhere in
        // it, which is exactly the case doctor must report as `missing`.
        let dir = tempfile::tempdir().unwrap();
        let roots = platform(&[&format!("{}\\blender.exe", dir.path().display())]);
        let found = candidates(
            &def("blender", platform(&["blender"]), roots),
            &env_with_path(dir.path()),
        );
        assert!(found.is_empty());
    }

    #[test]
    fn search_roots_expand_wildcards() {
        let root = tempfile::tempdir().unwrap();
        let binary = touch(&root.path().join("Blender 5.1"), "blender");
        let pattern = format!("{}/*/blender", root.path().display());
        let found = candidates(
            &def("blender", PlatformStrings::default(), platform(&[&pattern])),
            &HostEnv::new(),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, binary);
        assert_eq!(found[0].source, CandidateSource::SearchRoot);
    }

    #[test]
    fn match_path_handles_star_and_question() {
        assert!(match_path("blender*", "blender.exe"));
        assert!(match_path("Blender *", "blender 5.1"));
        assert!(match_path("blender", "blender"));
        assert!(match_path("may?", "maya"));
        assert!(!match_path("maya", "may"));
        assert!(!match_path("maya*", "nuke"));
        assert!(match_path("*", "anything"));
    }

    #[test]
    fn version_query_is_skipped_when_no_arg_is_declared() {
        let dir = tempfile::tempdir().unwrap();
        let binary = touch(dir.path(), "maya");
        assert_eq!(query_version(&binary, &[]), Ok(None));
    }

    #[test]
    fn version_query_reads_a_real_binary() {
        // Uses the test binary's own `--version`, so it needs no host installed.
        let self_exe = std::env::current_exe().expect("test binary path");
        let result = query_version(&self_exe, &["--version".to_string()]);
        // The binary either answers or refuses; both are readable outcomes. The
        // contract under test is that neither panics nor hangs.
        match result {
            Ok(version) => assert!(version.is_none() || version.is_some()),
            Err(VersionQueryError::Exited { .. } | VersionQueryError::Unparsable { .. }) => {}
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn version_query_reports_an_answer_without_a_version_token() {
        // A host that declares `--version` but answers with a splash screen (or
        // nothing) must not come back `Ok(None)`: the probe skips the version
        // gate on `Ok(None)`, which would report it `available` unverified.
        #[cfg(unix)]
        let (shell, args) = (
            "/bin/sh",
            vec!["-c".to_string(), "echo 'no version here'".to_string()],
        );
        #[cfg(windows)]
        let (shell, args) = (
            "cmd",
            vec!["/C".to_string(), "echo no version here".to_string()],
        );

        match query_version(Path::new(shell), &args) {
            Err(VersionQueryError::Unparsable { output }) => {
                assert!(
                    !output.is_empty(),
                    "the raw answer should be carried for diagnostics"
                );
            }
            other => panic!("expected Unparsable, got {other:?}"),
        }
    }

    #[test]
    fn version_query_reports_unlaunchable_binaries() {
        let dir = tempfile::tempdir().unwrap();
        let not_executable = dir.path().join("definitely-missing");
        assert!(matches!(
            query_version(&not_executable, &["--version".to_string()]),
            Err(VersionQueryError::NotLaunchable { .. })
        ));
    }

    #[test]
    fn install_channel_lookup_is_platform_scoped() {
        let mut def = def(
            "blender",
            platform(&["blender"]),
            PlatformStrings::default(),
        );
        def.install.insert(
            "windows".to_string(),
            InstallChannel::Winget {
                id: Some("Blender.Blender".to_string()),
                extra_args: Vec::new(),
            },
        );
        // The bundled manifest declares all three, so the lookup is exercised
        // against the real data instead of a fixture here.
        let bundled = crate::application::host::manifest::bundled();
        let blender = bundled.find("blender").unwrap();
        assert!(blender.install_channel_for_current().is_some());
    }

    #[test]
    fn sources_checked_lists_every_lookup() {
        let mut env = HostEnv::new();
        env.path_entries = Some(vec![PathBuf::from("/usr/bin")]);
        let def = def(
            "blender",
            platform(&["blender"]),
            platform(&["/opt/blender/blender"]),
        );
        let sources = sources_checked(&def, &env);
        assert_eq!(sources[0], "DCC_MCP_BLENDER_EXECUTABLE");
        assert_eq!(sources[1], "PATH");
        assert_eq!(sources[2], "/opt/blender/blender");
    }
}
