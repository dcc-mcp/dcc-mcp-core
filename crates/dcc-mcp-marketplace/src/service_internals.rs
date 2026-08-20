//! Internal helpers for [`super::MarketplaceService`] — atomic writes, source
//! resolution, git/zip install backends, filesystem utilities, and integrity
//! verification.  These are free functions shared across the service impl,
//! `bundle`, and `add_repo`.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use dcc_mcp_catalog::{self, CatalogEntry, CatalogInstall};
use semver::Version;
use sha2::{Digest, Sha256};

use crate::add_repo::collect_skill_dirs;
use crate::error::MarketplaceError;
use crate::git_command;
use crate::source::normalise_source;
use crate::types::{
    InstalledMarketplacePackage, MarketplaceSource, MarketplaceSourceConfig,
    MarketplaceSourceOrigin, entry_targets_dcc,
};

pub const ENV_MARKETPLACE_SOURCES: &str = "DCC_MCP_MARKETPLACE_SOURCES";
pub const ENV_MARKETPLACE_NO_DEFAULT_SOURCES: &str = "DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES";

// ── free functions ────────────────────────────────────────────────────────────

/// Serialises all marketplace file writes so concurrent callers (two
/// `save_config()` or two `save_installed_state()`) never share the same
/// temp file.  A `static Mutex` is fine here because the critical section
/// is disk I/O — tens of ms, not nanoseconds.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Atomic write — write to a temp file, sync, then rename into place.
///
/// Same pattern as `FileRegistry::write_atomic` in `dcc-mcp-transport`.
/// Callers are serialised by [`WRITE_LOCK`] so same-target-path writes
/// cannot clobber one another.
pub fn write_atomic(path: &Path, content: &str) -> Result<(), MarketplaceError> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let pid = std::process::id();
    let temp_path = dir.join(format!(".tmp.{pid}.marketplace.json"));

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp_path)
        .map_err(|err| {
            let _ = fs::remove_file(&temp_path);
            MarketplaceError::ConfigIo(temp_path.display().to_string(), err)
        })?;

    if let Err(err) = std::io::Write::write_all(&mut file, content.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(MarketplaceError::ConfigIo(
            temp_path.display().to_string(),
            err,
        ));
    }

    if let Err(err) = file.sync_data() {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(MarketplaceError::ConfigIo(
            temp_path.display().to_string(),
            err,
        ));
    }
    drop(file);

    const MAX_ATTEMPTS: u32 = 8;
    const BACKOFF_MS: u64 = 10;
    for attempt in 0..MAX_ATTEMPTS {
        match fs::rename(&temp_path, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                std::thread::sleep(std::time::Duration::from_millis(
                    BACKOFF_MS * (attempt as u64 + 1),
                ));
                if attempt == MAX_ATTEMPTS - 1 {
                    let _ = fs::remove_file(&temp_path);
                    return Err(MarketplaceError::ConfigIo(path.display().to_string(), e));
                }
            }
        }
    }
    unreachable!()
}

/// Check whether the `DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES` env var is set.
pub fn default_sources_disabled() -> bool {
    std::env::var(ENV_MARKETPLACE_NO_DEFAULT_SOURCES)
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

/// Parse sources from the `DCC_MCP_MARKETPLACE_SOURCES` env var (comma-separated).
pub fn env_sources() -> Vec<MarketplaceSource> {
    let Ok(value) = std::env::var(ENV_MARKETPLACE_SOURCES) else {
        return Vec::new();
    };
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| normalise_source(s, MarketplaceSourceOrigin::Env))
        .collect()
}

/// Validate a path component for safe filesystem use.
pub fn path_component(kind: &str, value: &str) -> Result<String, MarketplaceError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.starts_with('.')
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(MarketplaceError::InvalidPathComponent {
            kind: kind.to_string(),
            value: value.to_string(),
        });
    }
    Ok(trimmed.to_string())
}

pub fn load_config(path: &Path) -> Result<MarketplaceSourceConfig, MarketplaceError> {
    if !path.exists() {
        return Ok(MarketplaceSourceConfig::default());
    }
    let text = fs::read_to_string(path)
        .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
    serde_json::from_str(&text)
        .map_err(|err| MarketplaceError::ConfigParse(path.display().to_string(), err))
}

pub fn save_config(path: &Path, config: &MarketplaceSourceConfig) -> Result<(), MarketplaceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
    }
    let text = serde_json::to_string_pretty(config)
        .expect("MarketplaceSourceConfig serialization should not fail");
    write_atomic(path, &text)
}

pub fn resolve_install_dcc(
    entry: &CatalogEntry,
    requested: Option<&str>,
) -> Result<String, MarketplaceError> {
    if let Some(dcc) = requested {
        let dcc_name = path_component("DCC name", dcc)?.to_lowercase();
        if dcc_name == "any" {
            return Err(MarketplaceError::AmbiguousDcc {
                name: entry.name.clone(),
            });
        }
        if entry_targets_dcc(entry, &dcc_name) {
            return Ok(dcc_name);
        }
        return Err(MarketplaceError::DccMismatch {
            name: entry.name.clone(),
            dcc: dcc.to_string(),
        });
    }

    if entry.dcc.iter().any(|dcc| dcc.eq_ignore_ascii_case("any")) {
        return Err(MarketplaceError::AmbiguousDcc {
            name: entry.name.clone(),
        });
    }

    let mut dccs: Vec<String> = entry
        .dcc
        .iter()
        .map(|dcc| path_component("DCC name", dcc).map(|s| s.to_lowercase()))
        .collect::<Result<_, _>>()?;
    dccs.sort();
    dccs.dedup();
    match dccs.as_slice() {
        [dcc] => Ok(dcc.clone()),
        _ => Err(MarketplaceError::AmbiguousDcc {
            name: entry.name.clone(),
        }),
    }
}

pub fn ensure_entry_installable(entry: &CatalogEntry) -> Result<(), MarketplaceError> {
    if entry
        .policy
        .as_ref()
        .is_some_and(|policy| policy.installation == "not_available")
    {
        return Err(MarketplaceError::NotAvailable(entry.name.clone()));
    }
    let Some(required) = entry.min_core_version.as_deref() else {
        return Ok(());
    };
    let required_version =
        Version::parse(required).map_err(|_| MarketplaceError::InvalidMinCoreVersion {
            name: entry.name.clone(),
            required: required.to_string(),
        })?;
    let current = env!("CARGO_PKG_VERSION");
    let current_version =
        Version::parse(current).expect("workspace package version must be SemVer");
    if current_version < required_version {
        return Err(MarketplaceError::IncompatibleCoreVersion {
            name: entry.name.clone(),
            required: required.to_string(),
            current: current.to_string(),
        });
    }
    Ok(())
}

// ── install backends ─────────────────────────────────────────────────────────

pub fn install_from_git_command(
    install: &CatalogInstall,
    dest: &Path,
) -> Result<(), MarketplaceError> {
    let commit = required_git_commit(install)?;
    let url = install
        .url
        .as_deref()
        .ok_or_else(|| MarketplaceError::MissingInstall("git.url".into()))?;
    let output = git_command()
        .args(["init", "--quiet"])
        .arg(dest)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git init: {err}")))?;
    ensure_git_success("init", output)?;

    let output = git_command()
        .args(["remote", "add", "origin", url])
        .current_dir(dest)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git remote add: {err}")))?;
    ensure_git_success("remote add", output)?;

    let output = git_command()
        .args(["fetch", "--depth", "1", "origin", &commit])
        .current_dir(dest)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git fetch: {err}")))?;
    ensure_git_success("fetch pinned commit", output)?;

    let output = git_command()
        .args(["checkout", "--detach", "--quiet", "FETCH_HEAD"])
        .current_dir(dest)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git checkout: {err}")))?;
    ensure_git_success("checkout pinned commit", output)?;

    let actual = git_head_commit(dest).unwrap_or_else(|| "<unresolved>".into());
    if actual != commit {
        return Err(MarketplaceError::GitCommitMismatch {
            expected: commit,
            actual,
        });
    }
    Ok(())
}

fn ensure_git_success(
    operation: &str,
    output: std::process::Output,
) -> Result<(), MarketplaceError> {
    if output.status.success() {
        return Ok(());
    }
    Err(MarketplaceError::CommandFailed(format!(
        "git {operation} exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    )))
}

pub fn required_git_commit(install: &CatalogInstall) -> Result<String, MarketplaceError> {
    let reference = install.ref_.as_deref().unwrap_or_default().trim();
    if !is_full_git_oid(reference) {
        return Err(MarketplaceError::UnpinnedGitReference {
            reference: reference.to_string(),
        });
    }
    Ok(reference.to_ascii_lowercase())
}

pub fn immutable_git_commit(install: &CatalogInstall) -> Option<String> {
    if install.install_type != "git" {
        return None;
    }
    let ref_ = install.ref_.as_deref()?.trim();
    is_full_git_oid(ref_).then(|| ref_.to_ascii_lowercase())
}

pub fn resolved_git_commit(install: &CatalogInstall, dest: &Path) -> Option<String> {
    immutable_git_commit(install).or_else(|| git_head_commit(dest))
}

pub fn is_entry_outdated(
    entry: Option<&CatalogEntry>,
    installed: &InstalledMarketplacePackage,
) -> (bool, Option<String>) {
    let Some(entry) = entry else {
        return (false, None);
    };
    let version_changed = match (&entry.version, &installed.version) {
        (Some(latest), Some(current)) => latest != current,
        (Some(_), None) => true,
        (None, _) => false,
    };
    let latest_commit = entry.install.as_ref().and_then(immutable_git_commit);
    let commit_changed = latest_commit
        .as_deref()
        .is_some_and(|latest| installed.resolved_commit.as_deref() != Some(latest));
    (version_changed || commit_changed, latest_commit)
}

pub fn is_full_git_oid(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn git_head_commit(repo_path: &Path) -> Option<String> {
    let output = git_command()
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let revision = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_lowercase();
    is_full_git_oid(&revision).then_some(revision)
}

pub fn install_from_path(install: &CatalogInstall, dest: &Path) -> Result<(), MarketplaceError> {
    let url = install
        .url
        .as_deref()
        .ok_or_else(|| MarketplaceError::MissingInstall("path.url".into()))?;
    let src = url
        .strip_prefix("file://")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(url));
    if !src.join("SKILL.md").is_file() && collect_skill_dirs(&src).is_empty() {
        return Err(MarketplaceError::MissingSkill(src.display().to_string()));
    }
    copy_dir_recursive(&src, dest)
}

// ── zip / sha256 ─────────────────────────────────────────────────────────────

pub fn required_archive_sha256(install: &CatalogInstall) -> Result<String, MarketplaceError> {
    let url = install.url.as_deref().unwrap_or("<missing-url>");
    let Some(value) = install.sha256.as_deref() else {
        return Err(MarketplaceError::MissingArchiveChecksum { url: url.into() });
    };
    let normalized = normalize_sha256(value);
    if normalized.len() != 64 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(MarketplaceError::InvalidArchiveChecksum {
            value: value.to_string(),
        });
    }
    Ok(normalized)
}

pub fn validate_install_integrity(install: &CatalogInstall) -> Result<(), MarketplaceError> {
    match install.install_type.as_str() {
        "git" => required_git_commit(install).map(|_| ()),
        "zip" => required_archive_sha256(install).map(|_| ()),
        _ => Ok(()),
    }
}

pub fn verify_archive_sha256(
    bytes: &[u8],
    expected: &str,
    url: &str,
) -> Result<(), MarketplaceError> {
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(expected) {
        return Ok(());
    }
    Err(MarketplaceError::HashMismatch {
        url: url.to_string(),
        expected: expected.to_string(),
        actual,
    })
}

fn normalize_sha256(value: &str) -> String {
    value
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(value.trim())
        .to_ascii_lowercase()
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub fn extract_zip_archive(bytes: &[u8], dest: &Path) -> Result<(), MarketplaceError> {
    fs::create_dir_all(dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|err| MarketplaceError::Archive("zip".into(), err.to_string()))?;

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| MarketplaceError::Archive("zip".into(), err.to_string()))?;
        let Some(enclosed_name) = file.enclosed_name() else {
            return Err(MarketplaceError::Archive(
                file.name().to_string(),
                "archive entry escapes install root".into(),
            ));
        };
        let out_path = dest.join(enclosed_name);
        if file.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|err| MarketplaceError::ConfigIo(out_path.display().to_string(), err))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
            }
            let mut out_file = fs::File::create(&out_path)
                .map_err(|err| MarketplaceError::ConfigIo(out_path.display().to_string(), err))?;
            std::io::copy(&mut file, &mut out_file)
                .map_err(|err| MarketplaceError::ConfigIo(out_path.display().to_string(), err))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

/// If the extracted directory already has a SKILL.md at the top, nothing to do.
///
/// Otherwise, if there is exactly one child directory, flatten that archive
/// wrapper into `dest`.
pub fn flatten_single_skill_directory(dest: &Path) -> Result<(), MarketplaceError> {
    if dest.join("SKILL.md").is_file() {
        return Ok(());
    }

    let child_dirs: Vec<PathBuf> = fs::read_dir(dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_type = entry.file_type().ok()?;
            if file_type.is_dir() {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();

    let [child] = child_dirs.as_slice() else {
        return Ok(());
    };

    let flatten_root = dest.join(format!(".flattening-{}", now_ms()));
    fs::rename(child, &flatten_root)
        .map_err(|err| MarketplaceError::ConfigIo(flatten_root.display().to_string(), err))?;

    for entry in fs::read_dir(&flatten_root)
        .map_err(|err| MarketplaceError::ConfigIo(flatten_root.display().to_string(), err))?
    {
        let entry = entry
            .map_err(|err| MarketplaceError::ConfigIo(flatten_root.display().to_string(), err))?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        fs::rename(&from, &to).map_err(|err| {
            MarketplaceError::ConfigIo(format!("move {} -> {}", from.display(), to.display()), err)
        })?;
    }

    let _ = remove_path(&flatten_root);
    Ok(())
}

pub fn promote_single_nested_skill_directory(dest: &Path) -> Result<(), MarketplaceError> {
    if dest.join("SKILL.md").is_file() {
        return Ok(());
    }

    let skill_dirs = collect_skill_dirs(dest);
    let [skill_dir] = skill_dirs.as_slice() else {
        return Ok(());
    };
    if skill_dir == dest {
        return Ok(());
    }

    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let promoted = parent.join(format!(".promoting-{}", now_ms()));
    if promoted.exists() {
        remove_path(&promoted)?;
    }
    if let Err(err) = copy_dir_recursive(skill_dir, &promoted) {
        let _ = remove_path(&promoted);
        return Err(err);
    }
    remove_path(dest)?;
    fs::rename(&promoted, dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;
    Ok(())
}

// ── fs helpers ───────────────────────────────────────────────────────────────

pub fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), MarketplaceError> {
    fs::create_dir_all(dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;
    for entry in fs::read_dir(src)
        .map_err(|err| MarketplaceError::ConfigIo(src.display().to_string(), err))?
    {
        let entry =
            entry.map_err(|err| MarketplaceError::ConfigIo(src.display().to_string(), err))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|err| MarketplaceError::ConfigIo(src_path.display().to_string(), err))?;
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dest_path).map_err(|err| {
                MarketplaceError::ConfigIo(
                    format!("copy {} -> {}", src_path.display(), dest_path.display()),
                    err,
                )
            })?;
        }
    }
    Ok(())
}

pub fn remove_path(path: &Path) -> Result<(), MarketplaceError> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
