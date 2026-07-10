//! Free functions extracted from [`super::MarketplaceService`] —
//! atomic writes, git/zip backends, source helpers, fs utilities,
//! and integrity verification.
//!
//! Extracted from `service.rs` to stay under the 1500-line production
//! Rust gate while keeping the `MarketplaceService` impl block readable.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use dcc_mcp_catalog::{CatalogEntry, CatalogInstall};
use semver::Version;
use sha2::{Digest, Sha256};

use crate::add_repo::collect_skill_dirs;
use crate::error::MarketplaceError;
use crate::git_command;
use crate::source::normalise_source;
use crate::types::{
    MarketplaceSource, MarketplaceSourceConfig, MarketplaceSourceOrigin, entry_targets_dcc,
};

pub(super) const ENV_MARKETPLACE_SOURCES: &str = "DCC_MCP_MARKETPLACE_SOURCES";
pub(super) const ENV_MARKETPLACE_NO_DEFAULT_SOURCES: &str =
    "DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES";

// ── atomic write ───────────────────────────────────────────────────────────

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Atomic write — write to a temp file, sync, then rename into place.
pub(crate) fn write_atomic(path: &Path, content: &str) -> Result<(), MarketplaceError> {
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

// ── source helpers ─────────────────────────────────────────────────────────

/// Check whether the `DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES` env var is set.
pub fn default_sources_disabled() -> bool {
    std::env::var(ENV_MARKETPLACE_NO_DEFAULT_SOURCES)
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

/// Parse sources from the `DCC_MCP_MARKETPLACE_SOURCES` env var.
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

// ── config persistence ─────────────────────────────────────────────────────

pub(super) fn load_config(path: &Path) -> Result<MarketplaceSourceConfig, MarketplaceError> {
    if !path.exists() {
        return Ok(MarketplaceSourceConfig::default());
    }
    let text = fs::read_to_string(path)
        .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
    serde_json::from_str(&text)
        .map_err(|err| MarketplaceError::ConfigParse(path.display().to_string(), err))
}

pub(super) fn save_config(
    path: &Path,
    config: &MarketplaceSourceConfig,
) -> Result<(), MarketplaceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
    }
    let text = serde_json::to_string_pretty(config)
        .expect("MarketplaceSourceConfig serialization should not fail");
    write_atomic(path, &text)
}

// ── install resolution / compatibility ─────────────────────────────────────

pub(super) fn resolve_install_dcc(
    entry: &CatalogEntry,
    requested: Option<&str>,
) -> Result<String, MarketplaceError> {
    if let Some(dcc) = requested {
        let dcc_name = path_component("DCC name", dcc)?.to_lowercase();
        if entry_targets_dcc(entry, &dcc_name) {
            return Ok(dcc_name);
        }
        return Err(MarketplaceError::DccMismatch {
            name: entry.name.clone(),
            dcc: dcc.to_string(),
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

pub(super) fn ensure_core_version_compatible(entry: &CatalogEntry) -> Result<(), MarketplaceError> {
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

// ── install backends ───────────────────────────────────────────────────────

pub(super) fn install_from_git_command(
    install: &CatalogInstall,
    dest: &Path,
) -> Result<(), MarketplaceError> {
    let url = install
        .url
        .as_deref()
        .ok_or_else(|| MarketplaceError::MissingInstall("git.url".into()))?;
    let mut command = git_command();
    command.arg("clone").arg("--depth").arg("1");
    if let Some(ref_) = install.ref_.as_deref().filter(|v| !v.trim().is_empty()) {
        command.arg("--branch").arg(ref_);
    }
    command.arg(url).arg(dest);
    let output = command
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git clone: {err}")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(MarketplaceError::CommandFailed(format!(
        "git clone exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    )))
}

pub(super) fn immutable_git_commit(install: &CatalogInstall) -> Option<String> {
    if install.install_type != "git" {
        return None;
    }
    let ref_ = install.ref_.as_deref()?.trim();
    is_full_git_oid(ref_).then(|| ref_.to_ascii_lowercase())
}

pub(super) fn resolved_git_commit(install: &CatalogInstall, dest: &Path) -> Option<String> {
    immutable_git_commit(install).or_else(|| git_head_commit(dest))
}

pub(super) fn is_entry_outdated(
    entry: Option<&CatalogEntry>,
    installed: &crate::types::InstalledMarketplacePackage,
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

fn is_full_git_oid(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn git_head_commit(repo_path: &Path) -> Option<String> {
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

pub(super) fn github_archive_url(install: &CatalogInstall) -> Option<String> {
    let url = install.url.as_deref()?.trim().trim_end_matches('/');
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let mut parts = path.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim().trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    let ref_ = install
        .ref_
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("HEAD");
    Some(format!(
        "https://github.com/{owner}/{repo}/archive/{ref_}.zip"
    ))
}

pub(super) fn install_from_path(
    install: &CatalogInstall,
    dest: &Path,
) -> Result<(), MarketplaceError> {
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

pub(super) fn git_fetch_and_checkout(repo_path: &Path, ref_: &str) -> Result<(), MarketplaceError> {
    let output = git_command()
        .args(["fetch", "origin", "--tags"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git fetch: {err}")))?;
    if !output.status.success() {
        return Err(MarketplaceError::CommandFailed(format!(
            "git fetch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let output = git_command()
        .args(["checkout", ref_])
        .current_dir(repo_path)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git checkout: {err}")))?;
    if !output.status.success() {
        return Err(MarketplaceError::CommandFailed(format!(
            "git checkout failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

pub(super) fn git_pull(repo_path: &Path) -> Result<(), MarketplaceError> {
    let output = git_command()
        .args(["pull", "--ff-only"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git pull: {err}")))?;
    if !output.status.success() {
        return Err(MarketplaceError::CommandFailed(format!(
            "git pull failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

pub(super) fn git_remote_url(repo_path: &Path) -> Result<String, MarketplaceError> {
    let output = git_command()
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git remote get-url: {err}")))?;
    if !output.status.success() {
        return Err(MarketplaceError::CommandFailed(format!(
            "git remote get-url failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ── zip / sha256 ───────────────────────────────────────────────────────────

pub(super) fn verify_archive_sha256(
    bytes: &[u8],
    expected: Option<&str>,
    url: &str,
) -> Result<(), MarketplaceError> {
    let Some(expected) = expected
        .map(normalize_sha256)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(&expected) {
        return Ok(());
    }
    Err(MarketplaceError::HashMismatch {
        url: url.to_string(),
        expected,
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

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub(super) fn extract_zip_archive(bytes: &[u8], dest: &Path) -> Result<(), MarketplaceError> {
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

/// If the extracted directory already has SKILL.md at top, nothing to do.
/// Otherwise, if exactly one child dir, flatten that archive wrapper into `dest`.
pub(super) fn flatten_single_skill_directory(dest: &Path) -> Result<(), MarketplaceError> {
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

pub(crate) fn promote_single_nested_skill_directory(dest: &Path) -> Result<(), MarketplaceError> {
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

// ── fs helpers ─────────────────────────────────────────────────────────────

pub(crate) fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), MarketplaceError> {
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

pub(crate) fn remove_path(path: &Path) -> Result<(), MarketplaceError> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))
}

pub(super) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
