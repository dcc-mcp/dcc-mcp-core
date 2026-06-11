//! Direct GitHub repo installation — SKILL.md discovery, no marketplace.json needed.
//!
//! This module implements `dcc-mcp-cli marketplace add-repo <owner/repo>`:
//! - Clone a GitHub repo via `git clone --depth 1`
//! - Discover SKILL.md files to infer name/dcc/description
//! - Support `@subpath` syntax (vercel skills parity)
//! - `--list` to enumerate available skills without installing

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::MarketplaceError;
use crate::types::{RepoInstallResult, RepoSkillInfo, RepoSkillList};
use crate::path_component;

/// Parse a repo reference into a clone URL and optional subpath.
///
/// Supports:
/// - `owner/repo` → `https://github.com/owner/repo.git`
/// - `https://github.com/owner/repo` → as-is (with .git added if missing)
/// - `owner/repo@subdir` → clone URL + `subdir` subpath
/// - `https://github.com/owner/repo.git` → as-is
/// - `git@github.com:owner/repo.git` → as-is (SSH)
pub fn parse_repo_ref(raw: &str) -> Result<(String, Option<String>), MarketplaceError> {
    let trimmed = raw.trim();

    // SSH URL is detected first: git@host:path — the @ is part of the URL, not a subpath.
    if trimmed.starts_with("git@") {
        return Ok((ensure_git_suffix(trimmed), None));
    }

    // Split on last @ for subpath support (e.g., owner/repo@subdir).
    // Only applies to non-SSH URLs where @ separates a subpath.
    let (repo_part, subpath) = if let Some(at_pos) = trimmed.rfind('@') {
        let sub = &trimmed[at_pos + 1..];
        if sub.is_empty() || sub.contains('/') {
            return Err(MarketplaceError::CommandFailed(format!(
                "invalid repo subpath '{sub}': must be a single directory name"
            )));
        }
        (&trimmed[..at_pos], Some(sub.to_string()))
    } else {
        (trimmed, None)
    };

    let url = if looks_like_github_slug(repo_part) {
        format!("https://github.com/{repo_part}.git")
    } else if repo_part.starts_with("http://") || repo_part.starts_with("https://") {
        ensure_git_suffix(repo_part)
    } else {
        return Err(MarketplaceError::CommandFailed(format!(
            "invalid repo reference '{repo_part}': expected owner/repo or full URL"
        )));
    };

    Ok((url, subpath))
}

fn ensure_git_suffix(url: &str) -> String {
    if url.ends_with(".git") {
        url.to_string()
    } else {
        let no_trailing = url.trim_end_matches('/');
        format!("{no_trailing}.git")
    }
}

fn looks_like_github_slug(value: &str) -> bool {
    let Some((owner, repo)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty() && !repo.is_empty()
        && !value.contains("://")
        && !value.contains('\\')
        && !value.contains('@')
}

/// Clone a repo to a staging directory.
fn clone_repo(url: &str, dest: &Path) -> Result<(), MarketplaceError> {
    let output = Command::new("git")
        .args(["clone", "--depth", "1", url])
        .arg(dest)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git clone: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(MarketplaceError::CommandFailed(format!(
            "git clone exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

/// Parse a SKILL.md file and extract name, description, and dcc from frontmatter.
pub fn extract_skill_frontmatter(skill_dir: &Path) -> Option<RepoSkillInfo> {
    let skill_md = skill_dir.join("SKILL.md");
    let content = std::fs::read_to_string(skill_md).ok()?;

    // Extract YAML frontmatter between --- delimiters
    let frontmatter = extract_frontmatter(&content)?;

    // Parse the YAML frontmatter
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(frontmatter).ok()?;
    let mapping = value.as_mapping()?;

    let name = mapping
        .get(&serde_yaml_ng::Value::String("name".into()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;

    let description = mapping
        .get(&serde_yaml_ng::Value::String("description".into()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract dcc from metadata.dcc-mcp.dcc
    let dcc = mapping
        .get(&serde_yaml_ng::Value::String("metadata".into()))
        .and_then(|v| v.as_mapping())
        .and_then(|m| m.get(&serde_yaml_ng::Value::String("dcc-mcp".into())))
        .and_then(|v| v.as_mapping())
        .and_then(|m| m.get(&serde_yaml_ng::Value::String("dcc".into())))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(RepoSkillInfo {
        name,
        description,
        dcc,
        subpath: None,
    })
}

fn extract_frontmatter(content: &str) -> Option<&str> {
    const DELIMITER: &str = "---";
    if !content.starts_with(DELIMITER) {
        return None;
    }
    let after_first = &content[DELIMITER.len()..];
    let end = after_first.find("\n---")?;
    Some(after_first[..end].trim())
}

/// Collect all SKILL.md files under a root directory (shallow — one level).
fn collect_skill_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Check root
    if root.join("SKILL.md").is_file() {
        dirs.push(root.to_path_buf());
        return dirs;
    }

    // Check immediate subdirectories
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return dirs,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("SKILL.md").is_file() {
            dirs.push(path);
        }
    }

    dirs
}

/// A simple temp directory that auto-cleans up on drop.
struct StagingDir {
    path: PathBuf,
}

impl StagingDir {
    fn new() -> Result<Self, MarketplaceError> {
        let pid = std::process::id();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir_name = format!("dcc-mcp-add-repo-{pid}-{ts}");
        let path = std::env::temp_dir().join(&dir_name);
        std::fs::create_dir_all(&path)
            .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
        Ok(Self { path })
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ── high-level API ────────────────────────────────────────────────────────────

/// List available skills from a repo without installing.
///
/// Clones the repo, discovers SKILL.md files, parses frontmatter, and
/// returns the list. The clone directory is cleaned up before returning.
pub fn list_repo_skills(repo_ref: &str) -> Result<RepoSkillList, MarketplaceError> {
    let (url, subpath) = parse_repo_ref(repo_ref)?;

    let staging = StagingDir::new()?;
    let clone_dir = staging.path.clone();

    clone_repo(&url, &clone_dir)?;

    let search_root = match &subpath {
        Some(sub) => clone_dir.join(sub),
        None => clone_dir.clone(),
    };

    if subpath.is_some() && !search_root.exists() {
        return Err(MarketplaceError::CommandFailed(format!(
            "subpath '{}' not found in repo",
            subpath.as_deref().unwrap()
        )));
    }

    let skill_dirs = collect_skill_dirs(&search_root);
    let mut skills: Vec<RepoSkillInfo> = skill_dirs
        .iter()
        .filter_map(|dir| extract_skill_frontmatter(dir.as_path()))
        .collect();
    // Tag subpaths for non-root skills
    for skill in &mut skills {
        let dir = search_root
            .join(&skill.name);
        if dir.is_dir() && dir.join("SKILL.md").is_file() && dir.file_name().map(|n| n != ".").unwrap_or(false) {
            skill.subpath = dir.file_name().map(|n| n.to_string_lossy().to_string());
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));

    // StagingDir is cleaned up on drop

    Ok(RepoSkillList {
        repo_url: url,
        count: skills.len(),
        skills,
    })
}

/// Install a skill directly from a GitHub repo into the marketplace.
///
/// `repo_ref`: owner/repo, full URL, or @subpath variant.
/// `dcc`: explicit DCC override (required when SKILL.md doesn't declare one).
/// `force`: replace an existing installation.
/// `root`: the marketplace root directory (`~/.dcc-mcp/marketplace`).
pub fn install_from_repo(
    repo_ref: &str,
    dcc: Option<&str>,
    force: bool,
    root: &Path,
) -> Result<RepoInstallResult, MarketplaceError> {
    let (url, subpath) = parse_repo_ref(repo_ref)?;

    // Temp staging for clone
    let staging = StagingDir::new()?;
    let clone_dir = staging.path.clone();

    clone_repo(&url, &clone_dir)?;

    let search_root = match &subpath {
        Some(sub) => {
            let p = clone_dir.join(sub);
            if !p.exists() {
                return Err(MarketplaceError::CommandFailed(format!(
                    "subpath '{}' not found in repo",
                    sub
                )));
            }
            p
        }
        None => clone_dir.clone(),
    };

    let skill_dirs = collect_skill_dirs(&search_root);
    if skill_dirs.is_empty() {
        return Err(MarketplaceError::CommandFailed(
            "no SKILL.md found in repo".into(),
        ));
    }

    // Find the skill to install
    let target_skill = if skill_dirs.len() == 1 {
        // Single skill — use it directly
        let dir = &skill_dirs[0];
        let mut info = extract_skill_frontmatter(dir).ok_or_else(|| {
            MarketplaceError::CommandFailed(
                "failed to parse SKILL.md frontmatter".into(),
            )
        })?;
        info.subpath = if dir != &clone_dir {
            dir.file_name().map(|n| n.to_string_lossy().to_string())
        } else {
            None
        };
        info
    } else {
        // Multiple skills — try to resolve by --dcc or fail
        let all_skills: Vec<RepoSkillInfo> = skill_dirs
            .iter()
            .filter_map(|dir| extract_skill_frontmatter(dir.as_path()))
            .collect();

        match dcc {
            Some(requested_dcc) => {
                let matched = all_skills.into_iter().find(|s| {
                    s.dcc.as_deref().map(|d| d.eq_ignore_ascii_case(requested_dcc)).unwrap_or(false)
                }).ok_or_else(|| {
                    MarketplaceError::CommandFailed(format!(
                        "no skill for DCC '{requested_dcc}' found in repo; \
                         use --list to see available skills"
                    ))
                })?;
                matched
            }
            None => {
                return Err(MarketplaceError::CommandFailed(
                    "repo contains multiple skills; use --dcc <DCC> to select one, \
                     or --list to see available skills"
                        .into(),
                ))
            }
        }
    };

    // Determine the final DCC name
    let final_dcc = match dcc {
        Some(explicit) => path_component("DCC name", explicit)?.to_lowercase(),
        None => match &target_skill.dcc {
            Some(d) => path_component("DCC name", d)?.to_lowercase(),
            None => {
                return Err(MarketplaceError::CommandFailed(
                    "skill does not declare a DCC in SKILL.md; use --dcc to specify one"
                        .into(),
                ))
            }
        },
    };

    let package_name = path_component("package name", &target_skill.name)?;
    let dcc_root = root.join(&final_dcc);
    let dest = dcc_root.join(&package_name);

    if dest.exists() && !force {
        return Err(MarketplaceError::AlreadyInstalled {
            name: package_name.clone(),
            dcc: final_dcc.clone(),
            path: dest.display().to_string(),
        });
    }

    // Remove existing if forcing
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;
    }

    // Create parent dir and move staging into place
    std::fs::create_dir_all(&dcc_root)
        .map_err(|err| MarketplaceError::ConfigIo(dcc_root.display().to_string(), err))?;

    // Find the source directory — the skill dir within the clone
    let skill_source = if let Some(ref sp) = target_skill.subpath {
        search_root.join(sp)
    } else {
        skill_dirs
            .into_iter()
            .find(|d| extract_skill_frontmatter(d.as_path()).map(|s| s.name == target_skill.name).unwrap_or(false))
            .unwrap_or_else(|| clone_dir.clone())
    };

    // Copy the skill directory to destination
    copy_dir_recursive(&skill_source, &dest)?;

    Ok(RepoInstallResult {
        installed: true,
        name: package_name,
        dcc: final_dcc,
        repo_url: url,
        path: dest.display().to_string(),
        skill_search_path: dcc_root.display().to_string(),
        skill_subpath: target_skill.subpath,
        description: target_skill.description,
    })
}

/// Recursive directory copy, skipping .git directories.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), MarketplaceError> {
    std::fs::create_dir_all(dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;
    for entry in std::fs::read_dir(src)
        .map_err(|err| MarketplaceError::ConfigIo(src.display().to_string(), err))?
    {
        let entry = entry.map_err(|err| MarketplaceError::ConfigIo(src.display().to_string(), err))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|err| MarketplaceError::ConfigIo(src_path.display().to_string(), err))?;
        if file_type.is_dir() {
            // Skip .git directory
            if entry.file_name() == ".git" {
                continue;
            }
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dest_path).map_err(|err| {
                MarketplaceError::ConfigIo(
                    format!("copy {} -> {}", src_path.display(), dest_path.display()),
                    err,
                )
            })?;
        }
    }
    Ok(())
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_owner_repo() {
        let (url, subpath) = parse_repo_ref("dcc-mcp/dcc-mcp-maya").unwrap();
        assert_eq!(url, "https://github.com/dcc-mcp/dcc-mcp-maya.git");
        assert!(subpath.is_none());
    }

    #[test]
    fn parse_owner_repo_with_subpath() {
        let (url, subpath) = parse_repo_ref("dcc-mcp/dcc-mcp-maya@subdir").unwrap();
        assert_eq!(url, "https://github.com/dcc-mcp/dcc-mcp-maya.git");
        assert_eq!(subpath.unwrap(), "subdir");
    }

    #[test]
    fn parse_github_https_url() {
        let (url, subpath) = parse_repo_ref("https://github.com/dcc-mcp/dcc-mcp-maya.git").unwrap();
        assert_eq!(url, "https://github.com/dcc-mcp/dcc-mcp-maya.git");
        assert!(subpath.is_none());
    }

    #[test]
    fn parse_full_url_without_git_suffix() {
        let (url, subpath) =
            parse_repo_ref("https://github.com/dcc-mcp/dcc-mcp-maya").unwrap();
        assert_eq!(url, "https://github.com/dcc-mcp/dcc-mcp-maya.git");
        assert!(subpath.is_none());
    }

    #[test]
    fn parse_ssh_url() {
        let (url, subpath) =
            parse_repo_ref("git@github.com:dcc-mcp/dcc-mcp-maya.git").unwrap();
        assert_eq!(url, "git@github.com:dcc-mcp/dcc-mcp-maya.git");
        assert!(subpath.is_none());
    }

    #[test]
    fn parse_invalid_empty_subpath() {
        assert!(parse_repo_ref("owner/repo@").is_err());
    }

    #[test]
    fn parse_invalid_subpath_with_slash() {
        assert!(parse_repo_ref("owner/repo@sub/dir").is_err());
    }

    #[test]
    fn extract_frontmatter_basic() {
        let content = "---\nname: test-skill\ndescription: A test\n---\n";
        let info = extract_skill_frontmatter_from_str(content).unwrap();
        assert_eq!(info.name, "test-skill");
        assert_eq!(info.description.as_deref(), Some("A test"));
    }

    #[test]
    fn extract_frontmatter_with_dcc() {
        let content = "---\nname: maya-anim\ndescription: Maya animation tools\n\
            metadata:\n  dcc-mcp:\n    dcc: maya\n---\n";
        let info = extract_skill_frontmatter_from_str(content).unwrap();
        assert_eq!(info.name, "maya-anim");
        assert_eq!(info.description.as_deref(), Some("Maya animation tools"));
        assert_eq!(info.dcc.as_deref(), Some("maya"));
    }

    #[test]
    fn extract_frontmatter_no_dcc() {
        let content = "---\nname: generic-tool\ndescription: No DCC\n---\n";
        let info = extract_skill_frontmatter_from_str(content).unwrap();
        assert_eq!(info.name, "generic-tool");
        assert_eq!(info.dcc, None);
    }

    #[test]
    fn extract_frontmatter_no_frontmatter() {
        assert!(extract_skill_frontmatter_from_str("no frontmatter").is_none());
    }

    /// Helper to extract frontmatter from a string (used in tests).
    fn extract_skill_frontmatter_from_str(content: &str) -> Option<RepoSkillInfo> {
        let fm = extract_frontmatter(content)?;
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(fm).ok()?;
        let mapping = value.as_mapping()?;

        let name = mapping
            .get(&serde_yaml_ng::Value::String("name".into()))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?;

        let description = mapping
            .get(&serde_yaml_ng::Value::String("description".into()))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let dcc = mapping
            .get(&serde_yaml_ng::Value::String("metadata".into()))
            .and_then(|v| v.as_mapping())
            .and_then(|m| m.get(&serde_yaml_ng::Value::String("dcc-mcp".into())))
            .and_then(|v| v.as_mapping())
            .and_then(|m| m.get(&serde_yaml_ng::Value::String("dcc".into())))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(RepoSkillInfo {
            name,
            description,
            dcc,
            subpath: None,
        })
    }

    #[test]
    fn write_skill_md_and_extract() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: My cool skill\n\
             metadata:\n  dcc-mcp:\n    dcc: blender\n---\n",
        )
        .unwrap();

        let info = extract_skill_frontmatter(&skill_dir).unwrap();
        assert_eq!(info.name, "my-skill");
        assert_eq!(info.description.as_deref(), Some("My cool skill"));
        assert_eq!(info.dcc.as_deref(), Some("blender"));
    }
}
