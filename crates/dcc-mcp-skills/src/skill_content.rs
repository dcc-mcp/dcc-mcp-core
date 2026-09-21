//! `io.modelcontextprotocol/skills` content plumbing.
//!
//! This module owns everything the Skills **transport binding** needs from the
//! skill format, and nothing about JSON-RPC:
//!
//! - [`SKILL_URI_SCHEME`] URI construction and parsing (`skill://<name>/<path>`).
//! - [`SkillFrontmatter`] — the verbatim YAML frontmatter of a `SKILL.md`,
//!   rendered as a JSON object.
//! - [`SkillManifest`] — a complete `{uri, digest, size}` enumeration of every
//!   file in a skill directory, computed from the bytes actually served.
//!
//! The wire types and the MCP method handlers live one layer up
//! (`dcc-mcp-jsonrpc` and the HTTP server's stateless service) so this crate
//! stays transport-neutral.
//!
//! Reference: <https://github.com/modelcontextprotocol/ext-skills/blob/main/specification/stable/skills.mdx>

use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::constants::SKILL_METADATA_FILE;

/// URI scheme used for skill resources served by this server.
pub const SKILL_URI_SCHEME: &str = "skill";

/// Prefix of every skill resource URI, including the scheme delimiter.
pub const SKILL_URI_PREFIX: &str = "skill://";

/// Per-skill cap on the number of files a conforming host must accept.
///
/// Source: ext-skills specification, "Limits".
pub const MAX_SKILL_RESOURCE_ENTRIES: usize = 512;

/// Per-skill cap on total file bytes a conforming host must accept
/// (16 MiB = 16,777,216 bytes).
///
/// Source: ext-skills specification, "Limits".
pub const MAX_SKILL_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

/// `mimeType` reported for directory resources (`resources/directory/read`).
pub const DIRECTORY_MIME_TYPE: &str = "inode/directory";

/// `mimeType` reported for a skill's `SKILL.md`.
pub const SKILL_MD_MIME_TYPE: &str = "text/markdown";

/// Why a skill directory could not be turned into a publishable manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillManifestError {
    /// The skill directory has no readable `SKILL.md`.
    MissingSkillMd,
    /// `SKILL.md` has no `---` delimited frontmatter block.
    MissingFrontmatter,
    /// The frontmatter block is not a YAML mapping, or is not valid YAML.
    InvalidFrontmatter(String),
    /// A file inside the skill directory could not be read or hashed.
    UnreadableFile { relative: String, reason: String },
    /// The skill exceeds [`MAX_SKILL_RESOURCE_ENTRIES`] or
    /// [`MAX_SKILL_TOTAL_BYTES`], so no conforming host is obliged to load it.
    ExceedsLimits { entries: usize, total_bytes: u64 },
}

impl std::fmt::Display for SkillManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSkillMd => write!(f, "SKILL.md is missing or unreadable"),
            Self::MissingFrontmatter => write!(f, "SKILL.md has no YAML frontmatter block"),
            Self::InvalidFrontmatter(reason) => write!(f, "invalid SKILL.md frontmatter: {reason}"),
            Self::UnreadableFile { relative, reason } => {
                write!(f, "cannot read skill file `{relative}`: {reason}")
            }
            Self::ExceedsLimits {
                entries,
                total_bytes,
            } => write!(
                f,
                "skill exceeds the ext-skills limits ({entries} files / {total_bytes} bytes)"
            ),
        }
    }
}

impl std::error::Error for SkillManifestError {}

/// The YAML frontmatter of a `SKILL.md`, rendered verbatim as JSON.
///
/// The Skills extension requires the entry's `frontmatter` object to be
/// identical in content to the frontmatter of the `SKILL.md` it describes:
/// every field the author wrote, not a curated subset. This is therefore kept
/// as a raw JSON map instead of the normalized [`crate::SkillMetadata`] model,
/// which drops unknown keys and injects defaults.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SkillFrontmatter(Map<String, Value>);

impl SkillFrontmatter {
    /// Borrow the underlying JSON object.
    #[must_use]
    pub fn as_object(&self) -> &Map<String, Value> {
        &self.0
    }

    /// Consume into the underlying JSON object.
    #[must_use]
    pub fn into_object(self) -> Map<String, Value> {
        self.0
    }

    /// The skill `name` as declared in frontmatter, when present.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.0.get("name").and_then(Value::as_str)
    }

    /// The skill `description` as declared in frontmatter, when present.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.0.get("description").and_then(Value::as_str)
    }
}

/// One file of a skill, with the digest and size of the bytes served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFileEntry {
    /// Resource URI of the file.
    pub uri: String,
    /// `sha256:{hex}` digest of the file's raw bytes.
    pub digest: String,
    /// Length in bytes of the raw content the digest covers.
    pub size: u64,
    /// Path of the file relative to the skill directory, using `/`.
    pub relative_path: String,
}

/// A skill's files: either a complete enumeration or the `"dynamic"` marker.
///
/// `"dynamic"` is reserved for skills whose content is generated such that
/// stable digests cannot be published. dcc-mcp serves skills straight from
/// disk, so [`Self::Files`] is the only variant this crate produces; the
/// marker exists so the wire type round-trips faithfully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillFileSet {
    /// Complete enumeration of `SKILL.md` and every supporting file.
    Files(Vec<SkillFileEntry>),
    /// Content is generated; no stable digests can be published.
    Dynamic,
}

/// Everything the Skills extension publishes about one skill.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillManifest {
    /// Resource URI of the skill's `SKILL.md`.
    pub uri: String,
    /// Verbatim frontmatter of that `SKILL.md`.
    pub frontmatter: SkillFrontmatter,
    /// Complete file enumeration, or the dynamic marker.
    pub resources: SkillFileSet,
}

/// Build the resource URI of a file inside a skill.
///
/// `relative` uses `/` separators and is interpreted relative to the skill
/// directory root; pass [`SKILL_METADATA_FILE`] for the skill's own `SKILL.md`.
#[must_use]
pub fn skill_uri(skill_name: &str, relative: &str) -> String {
    let relative = relative.trim_start_matches('/');
    if relative.is_empty() {
        format!("{SKILL_URI_PREFIX}{skill_name}")
    } else {
        format!("{SKILL_URI_PREFIX}{skill_name}/{relative}")
    }
}

/// Split a skill resource URI into `(skill name, relative path)`.
///
/// Returns `None` when the URI is not a `skill://` URI, or when it carries no
/// skill name. The relative path is empty for the skill's root directory URI.
#[must_use]
pub fn parse_skill_uri(uri: &str) -> Option<(&str, &str)> {
    let rest = uri.strip_prefix(SKILL_URI_PREFIX)?;
    let rest = rest.split('#').next().unwrap_or(rest);
    let rest = rest.split('?').next().unwrap_or(rest);
    if rest.is_empty() {
        return None;
    }
    let (name, relative) = match rest.split_once('/') {
        Some((name, relative)) => (name, relative),
        None => (rest, ""),
    };
    if name.is_empty() {
        return None;
    }
    Some((name, relative.trim_start_matches('/')))
}

/// Parse the YAML frontmatter of a `SKILL.md` into a verbatim JSON object.
///
/// Returns `None` when the content has no `---` delimited frontmatter block,
/// or when that block is not a YAML mapping.
#[must_use]
pub fn skill_frontmatter(skill_md: &str) -> Option<SkillFrontmatter> {
    let yaml = extract_frontmatter(skill_md)?;
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).ok()?;
    let mapping = value.as_mapping()?;
    let mut object = Map::new();
    for (key, value) in mapping {
        let key = key.as_str()?.to_string();
        let json = serde_json::to_value(value).ok()?;
        object.insert(key, json);
    }
    Some(SkillFrontmatter(object))
}

/// Extract the YAML frontmatter block as a borrowed slice.
///
/// Mirrors the loader's private helper: the block starts at the first byte and
/// ends at the first line that begins with `---`.
fn extract_frontmatter(content: &str) -> Option<&str> {
    const DELIMITER: &str = "---";
    if !content.starts_with(DELIMITER) {
        return None;
    }
    let after_first = &content[DELIMITER.len()..];
    let end = after_first.find("\n---")?;
    let block = after_first[..end].trim();
    if block.is_empty() {
        return None;
    }
    Some(block)
}

/// Build the publishable manifest of a skill directory.
///
/// Walks `skill_dir`, hashes every regular file it finds, and returns entries
/// sorted by URI so the manifest is stable across calls. The digest and size
/// always describe the bytes `resources/read` will serve.
///
/// `skill_name` is the catalog key for the skill and becomes the first URI
/// path segment; the Skills extension requires it to equal `frontmatter.name`.
///
/// Directory entries, symlinked directories, and files that cannot be read are
/// handled as follows: unreadable files fail the whole manifest (a manifest
/// with a missing entry would not be complete), while symlinked directories are
/// not followed so a skill cannot escape its own root.
pub fn build_skill_manifest(
    skill_dir: &Path,
    skill_name: &str,
) -> Result<SkillManifest, SkillManifestError> {
    let mut files = Vec::new();
    collect_files(skill_dir, skill_dir, &mut files)?;

    let skill_md = files
        .iter()
        .find(|(relative, _)| relative == SKILL_METADATA_FILE)
        .map(|(_, path)| path.clone())
        .ok_or(SkillManifestError::MissingSkillMd)?;
    let skill_md_bytes =
        std::fs::read(&skill_md).map_err(|_| SkillManifestError::MissingSkillMd)?;
    let skill_md_text = String::from_utf8(skill_md_bytes.clone())
        .map_err(|_| SkillManifestError::MissingSkillMd)?;
    let frontmatter =
        skill_frontmatter(&skill_md_text).ok_or(SkillManifestError::MissingFrontmatter)?;
    if frontmatter.name() != Some(skill_name) {
        return Err(SkillManifestError::InvalidFrontmatter(format!(
            "frontmatter name `{}` does not match the skill key `{skill_name}`",
            frontmatter.name().unwrap_or("<missing>")
        )));
    }
    if frontmatter.description().is_none() {
        return Err(SkillManifestError::InvalidFrontmatter(
            "frontmatter is missing the required `description` field".to_string(),
        ));
    }

    let mut entries = Vec::with_capacity(files.len());
    let mut total_bytes: u64 = 0;
    for (relative, path) in &files {
        let bytes = std::fs::read(path).map_err(|error| SkillManifestError::UnreadableFile {
            relative: relative.clone(),
            reason: error.to_string(),
        })?;
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        total_bytes = total_bytes.saturating_add(size);
        entries.push(SkillFileEntry {
            uri: skill_uri(skill_name, relative),
            digest: digest_bytes(&bytes),
            size,
            relative_path: relative.clone(),
        });
    }
    entries.sort_by(|a, b| a.uri.cmp(&b.uri));

    if entries.len() > MAX_SKILL_RESOURCE_ENTRIES || total_bytes > MAX_SKILL_TOTAL_BYTES {
        return Err(SkillManifestError::ExceedsLimits {
            entries: entries.len(),
            total_bytes,
        });
    }

    Ok(SkillManifest {
        uri: skill_uri(skill_name, SKILL_METADATA_FILE),
        frontmatter,
        resources: SkillFileSet::Files(entries),
    })
}

/// Collect regular files under `root` as `(relative path, absolute path)`.
///
/// Symlinked directories are not followed: a skill must not be able to pull
/// content from outside its own root into its manifest.
fn collect_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), SkillManifestError> {
    let read_dir = std::fs::read_dir(dir).map_err(|error| SkillManifestError::UnreadableFile {
        relative: relative_key(root, dir),
        reason: error.to_string(),
    })?;
    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                return Err(SkillManifestError::UnreadableFile {
                    relative: relative_key(root, dir),
                    reason: error.to_string(),
                });
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                return Err(SkillManifestError::UnreadableFile {
                    relative: relative_key(root, &path),
                    reason: error.to_string(),
                });
            }
        };
        if file_type.is_symlink() {
            // A symlinked directory could walk the tree outside the skill root;
            // a symlinked file is followed only when its target stays regular.
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            if metadata.is_dir() {
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
        } else if file_type.is_dir() {
            collect_files(root, &path, out)?;
            continue;
        } else if !file_type.is_file() {
            continue;
        }
        out.push((relative_key(root, &path), path));
    }
    Ok(())
}

/// Render `path` relative to `root` with `/` separators.
fn relative_key(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// SHA-256 of `bytes`, formatted as `sha256:{lowercase hex}`.
#[must_use]
pub fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        // Writing to a `String` cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    format!("sha256:{hex}")
}

/// Resolve a `skill://` file URI to a path inside `skill_dir`.
///
/// Returns `None` when `relative` escapes the skill root (contains `..` or is
/// absolute) so that a crafted URI cannot read outside the skill.
#[must_use]
pub fn resolve_skill_file(skill_dir: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() || relative.starts_with('/') || relative.contains('\0') {
        return None;
    }
    let mut resolved = skill_dir.to_path_buf();
    for segment in relative.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return None,
            segment => resolved.push(segment),
        }
    }
    if resolved == skill_dir {
        return None;
    }
    Some(resolved)
}

/// Guess the `mimeType` for a skill file from its extension.
///
/// Markdown is the only type the Skills extension calls out; everything else
/// falls back to `application/octet-stream` so a host never has to guess.
#[must_use]
pub fn mime_type_for(relative_path: &str) -> &'static str {
    let extension = relative_path
        .rsplit('.')
        .next()
        .filter(|_| relative_path.contains('.'));
    match extension {
        Some(ext) if ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown") => {
            "text/markdown"
        }
        Some(ext) if ext.eq_ignore_ascii_case("txt") => "text/plain",
        Some(ext) if ext.eq_ignore_ascii_case("json") => "application/json",
        Some(ext) if ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml") => {
            "application/yaml"
        }
        Some(ext) if ext.eq_ignore_ascii_case("py") => "text/x-python",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn skill_uri_builds_and_parses_round_trip() {
        assert_eq!(skill_uri("demo", "SKILL.md"), "skill://demo/SKILL.md");
        assert_eq!(
            skill_uri("demo", "references/GUIDE.md"),
            "skill://demo/references/GUIDE.md"
        );
        assert_eq!(skill_uri("demo", ""), "skill://demo");
        assert_eq!(skill_uri("demo", "/SKILL.md"), "skill://demo/SKILL.md");
        assert_eq!(
            parse_skill_uri("skill://demo/references/GUIDE.md"),
            Some(("demo", "references/GUIDE.md"))
        );
        assert_eq!(parse_skill_uri("skill://demo"), Some(("demo", "")));
    }

    #[test]
    fn parse_skill_uri_rejects_non_skill_uris() {
        assert_eq!(parse_skill_uri("file:///tmp/SKILL.md"), None);
        assert_eq!(parse_skill_uri("skill://"), None);
        assert_eq!(parse_skill_uri("skill:///SKILL.md"), None);
        assert_eq!(parse_skill_uri(""), None);
    }

    #[test]
    fn digest_matches_the_sha256_hex_form() {
        // Known vector: SHA-256 of the empty string.
        assert_eq!(
            digest_bytes(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // Known vector: SHA-256 of "abc".
        assert_eq!(
            digest_bytes(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn frontmatter_is_kept_verbatim() {
        let content = "---\nname: demo\ndescription: Demo skill\nlicense: MIT\nmetadata:\n  author: studio\n  nested:\n    flag: true\n---\n\n# Demo\n";
        let frontmatter = skill_frontmatter(content).expect("frontmatter parses");
        assert_eq!(frontmatter.name(), Some("demo"));
        assert_eq!(frontmatter.description(), Some("Demo skill"));
        let object = frontmatter.into_object();
        // Unknown-to-dcc-mcp keys must survive untouched.
        assert_eq!(object["license"], Value::String("MIT".into()));
        assert_eq!(object["metadata"]["author"], Value::String("studio".into()));
        assert_eq!(object["metadata"]["nested"]["flag"], Value::Bool(true));
    }

    #[test]
    fn frontmatter_rejects_missing_or_non_mapping_blocks() {
        assert!(skill_frontmatter("# No frontmatter\n").is_none());
        assert!(skill_frontmatter("---\n- just\n- a list\n---\n").is_none());
        assert!(skill_frontmatter("---\n---\n").is_none());
    }

    #[test]
    fn manifest_enumerates_every_file_with_digests() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n\nbody\n",
        );
        write(&root.join("references/GUIDE.md"), "guide\n");
        write(&root.join("scripts/run.py"), "print(1)\n");

        let manifest = build_skill_manifest(root, "demo").expect("manifest builds");
        assert_eq!(manifest.uri, "skill://demo/SKILL.md");
        let files = match &manifest.resources {
            SkillFileSet::Files(files) => files.clone(),
            SkillFileSet::Dynamic => panic!("disk-backed skills are never dynamic"),
        };
        assert_eq!(files.len(), 3);
        // Sorted by URI so the manifest is stable.
        assert_eq!(files[0].uri, "skill://demo/SKILL.md");
        assert_eq!(files[1].uri, "skill://demo/references/GUIDE.md");
        assert_eq!(files[2].uri, "skill://demo/scripts/run.py");
        // Size and digest describe the exact bytes on disk.
        let guide = &files[1];
        assert_eq!(guide.size, 6);
        assert_eq!(guide.digest, digest_bytes(b"guide\n"));
    }

    #[test]
    fn manifest_rejects_name_mismatch_and_missing_description() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("SKILL.md"),
            "---\nname: other\ndescription: Demo\n---\n",
        );
        assert!(matches!(
            build_skill_manifest(dir.path(), "demo"),
            Err(SkillManifestError::InvalidFrontmatter(_))
        ));

        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("SKILL.md"), "---\nname: demo\n---\n");
        assert!(matches!(
            build_skill_manifest(dir.path(), "demo"),
            Err(SkillManifestError::InvalidFrontmatter(_))
        ));
    }

    #[test]
    fn manifest_rejects_missing_skill_md() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("README.md"), "no skill here\n");
        assert_eq!(
            build_skill_manifest(dir.path(), "demo"),
            Err(SkillManifestError::MissingSkillMd)
        );
    }

    #[test]
    fn manifest_rejects_skills_over_the_resource_count_limit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        );
        for index in 0..MAX_SKILL_RESOURCE_ENTRIES {
            write(&root.join(format!("file-{index}.md")), "x");
        }
        assert!(matches!(
            build_skill_manifest(root, "demo"),
            Err(SkillManifestError::ExceedsLimits { .. })
        ));
    }

    #[test]
    fn manifest_accepts_a_skill_exactly_at_the_limits() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // One byte of body plus the frontmatter block; the count is what matters.
        write(
            &root.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        );
        for index in 1..MAX_SKILL_RESOURCE_ENTRIES {
            write(&root.join(format!("file-{index}.md")), "x");
        }
        let manifest = build_skill_manifest(root, "demo").expect("at the limit is allowed");
        let files = match manifest.resources {
            SkillFileSet::Files(files) => files,
            SkillFileSet::Dynamic => panic!("disk-backed skills are never dynamic"),
        };
        assert_eq!(files.len(), MAX_SKILL_RESOURCE_ENTRIES);
    }

    #[test]
    fn resolve_skill_file_refuses_to_escape_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(
            resolve_skill_file(root, "references/GUIDE.md"),
            Some(root.join("references/GUIDE.md"))
        );
        assert_eq!(resolve_skill_file(root, "../SKILL.md"), None);
        assert_eq!(resolve_skill_file(root, "a/../../SKILL.md"), None);
        assert_eq!(resolve_skill_file(root, "/etc/passwd"), None);
        assert_eq!(resolve_skill_file(root, ""), None);
    }

    #[test]
    fn mime_types_cover_the_common_skill_file_kinds() {
        assert_eq!(mime_type_for("SKILL.md"), "text/markdown");
        assert_eq!(mime_type_for("notes.txt"), "text/plain");
        assert_eq!(mime_type_for("tools.yaml"), "application/yaml");
        assert_eq!(mime_type_for("run.py"), "text/x-python");
        assert_eq!(mime_type_for("binary.bin"), "application/octet-stream");
    }
}
