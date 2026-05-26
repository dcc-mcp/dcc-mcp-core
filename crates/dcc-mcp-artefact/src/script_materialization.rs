//! Per-instance script materialization store (#1220).
//!
//! The store writes ad-hoc script content to a host-local path before a DCC
//! interpreter runs it. The descriptor preserves the DCC type, live instance,
//! MCP session, request metadata, and a [`FileRef`] compatible URI so audit,
//! replay, and sandbox allowlists can reason about a concrete file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{FileRef, atomic_write_bytes, hash_bytes_sha256};

/// Environment override for the default materialization root.
pub const SCRIPT_MATERIALIZATION_ROOT_ENV: &str = "DCC_MCP_SCRIPT_MATERIALIZATION_ROOT";

/// Request for materializing one script payload.
#[derive(Debug, Clone)]
pub struct ScriptMaterializationRequest {
    pub content: String,
    pub dcc_type: String,
    pub instance_id: String,
    pub session_id: String,
    pub language: String,
    pub suffix: String,
    pub display_name: Option<String>,
    pub reuse: bool,
    pub reuse_key: Option<String>,
    pub ttl_secs: Option<u64>,
    pub tool_call_id: Option<String>,
    pub correlation_id: Option<String>,
}

impl ScriptMaterializationRequest {
    /// Build a request with Python-friendly defaults.
    pub fn new(
        content: impl Into<String>,
        dcc_type: impl Into<String>,
        instance_id: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            content: content.into(),
            dcc_type: dcc_type.into(),
            instance_id: instance_id.into(),
            session_id: session_id.into(),
            language: "python".to_string(),
            suffix: ".py".to_string(),
            display_name: None,
            reuse: false,
            reuse_key: None,
            ttl_secs: None,
            tool_call_id: None,
            correlation_id: None,
        }
    }
}

/// Structured descriptor returned by [`ScriptMaterializationStore`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterializedScript {
    pub file_ref: FileRef,
    pub file_path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
    pub language: String,
    pub suffix: String,
    pub dcc_type: String,
    pub instance_id: String,
    pub session_id: String,
    pub script_id: String,
    pub ttl_secs: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub tool_call_id: Option<String>,
    pub correlation_id: Option<String>,
    pub reused: bool,
}

impl MaterializedScript {
    /// Return true when this descriptor is expired at ``now``.
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
    }
}

/// Errors returned by script materialization.
#[must_use]
#[derive(Debug, thiserror::Error)]
pub enum ScriptMaterializationError {
    #[error("invalid materialization request: {0}")]
    InvalidRequest(String),
    #[error("materialized script path escapes root: {0}")]
    PathEscape(String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type ScriptMaterializationResult<T> = Result<T, ScriptMaterializationError>;

/// Store rooted at ``~/.dcc-mcp`` or ``DCC_MCP_SCRIPT_MATERIALIZATION_ROOT``.
#[derive(Debug, Clone)]
pub struct ScriptMaterializationStore {
    root: PathBuf,
}

impl ScriptMaterializationStore {
    /// Create a store under an explicit root.
    pub fn new_in(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Create a store under the configured default root.
    pub fn new_default() -> Self {
        Self::new_in(default_script_materialization_root())
    }

    /// Return the configured root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Materialize one script and return its descriptor.
    pub fn materialize(
        &self,
        request: ScriptMaterializationRequest,
    ) -> ScriptMaterializationResult<MaterializedScript> {
        validate_ttl(request.ttl_secs)?;
        let bytes = request.content.as_bytes();
        let sha256 = hash_bytes_sha256(bytes);
        let suffix = normalize_suffix(&request.suffix);
        let language = sanitize_segment(&request.language, "text").to_ascii_lowercase();
        let dcc_type = sanitize_segment(&request.dcc_type, "unknown");
        let instance_id = sanitize_segment(&request.instance_id, "unknown");
        let session_id = sanitize_segment(&request.session_id, "unknown");
        let script_id = script_id_for(
            &sha256,
            request.reuse,
            request.reuse_key.as_deref(),
            request.display_name.as_deref().unwrap_or("script"),
        );

        let target_dir = self
            .root
            .join(&dcc_type)
            .join("temp")
            .join(&instance_id)
            .join(&session_id);
        fs::create_dir_all(&target_dir)?;
        ensure_within_root(&target_dir, &self.root)?;
        let file_path = target_dir.join(format!("{script_id}{suffix}"));
        ensure_within_root(&file_path, &self.root)?;
        let metadata_path = metadata_path_for(&file_path);

        if request.reuse
            && file_path.is_file()
            && metadata_path.is_file()
            && let Some(existing) = read_descriptor_lenient(&metadata_path)?
            && existing.sha256 == sha256
            && !existing.is_expired_at(Utc::now())
        {
            return Ok(MaterializedScript {
                reused: true,
                ..existing
            });
        }

        let now = Utc::now();
        let expires_at = expires_at_from_ttl(request.ttl_secs);
        let file_ref = FileRef {
            uri: file_uri_from_path(&file_path),
            mime: Some(mime_for_language(&language).to_string()),
            size_bytes: Some(bytes.len() as u64),
            display_name: request.display_name.clone().or_else(|| {
                file_path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            }),
            digest: Some(format!("sha256:{sha256}")),
            producer_job_id: None,
            tool_call_id: request.tool_call_id.clone(),
            session_id: Some(session_id.clone()),
            correlation_id: request.correlation_id.clone(),
            created_at: now,
            expires_at,
            metadata: serde_json::json!({
                "materialization_kind": "script",
                "dcc_type": dcc_type,
                "instance_id": instance_id,
                "session_id": session_id,
                "script_id": script_id,
                "language": language,
                "suffix": suffix,
            }),
        };
        let descriptor = MaterializedScript {
            file_ref,
            file_path,
            sha256,
            bytes: bytes.len() as u64,
            language,
            suffix,
            dcc_type,
            instance_id,
            session_id,
            script_id,
            ttl_secs: request.ttl_secs,
            created_at: now,
            expires_at,
            tool_call_id: request.tool_call_id,
            correlation_id: request.correlation_id,
            reused: false,
        };

        atomic_write_bytes(&descriptor.file_path, bytes, true)?;
        atomic_write_bytes(
            &metadata_path,
            &serde_json::to_vec(&descriptor).map_err(ScriptMaterializationError::Serde)?,
            true,
        )?;
        Ok(descriptor)
    }

    /// Remove expired scripts and metadata sidecars. Returns removed file count.
    pub fn cleanup_expired(&self, now: DateTime<Utc>) -> ScriptMaterializationResult<usize> {
        cleanup_materialized_scripts(&self.root, now, false)
    }

    /// Remove all scripts and metadata sidecars below the store root.
    pub fn cleanup_all(&self) -> ScriptMaterializationResult<usize> {
        cleanup_materialized_scripts(&self.root, Utc::now(), true)
    }
}

/// Return the default materialization root.
pub fn default_script_materialization_root() -> PathBuf {
    if let Some(root) = std::env::var_os(SCRIPT_MATERIALIZATION_ROOT_ENV) {
        return PathBuf::from(root);
    }
    home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".dcc-mcp")
}

/// Sanitize one path segment for DCC, instance, session, and script ids.
pub fn sanitize_segment(value: &str, default: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches(['.', '_', '-']).to_string();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return default.to_string();
    }
    trimmed.chars().take(96).collect()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn validate_ttl(ttl_secs: Option<u64>) -> ScriptMaterializationResult<()> {
    if ttl_secs == Some(0) {
        return Err(ScriptMaterializationError::InvalidRequest(
            "ttl_secs must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

fn normalize_suffix(suffix: &str) -> String {
    let raw = if suffix.trim().is_empty() {
        ".py"
    } else {
        suffix.trim()
    };
    let mut out = String::new();
    out.push('.');
    let trimmed = raw.trim_start_matches('.');
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out == "." || out == ".." {
        return ".txt".to_string();
    }
    out.chars().take(32).collect()
}

fn script_id_for(sha256: &str, reuse: bool, reuse_key: Option<&str>, prefix: &str) -> String {
    if reuse {
        if let Some(reuse_key) = reuse_key {
            return format!("{}_{}", sanitize_segment(reuse_key, "reuse"), &sha256[..12]);
        }
        return sha256.to_string();
    }
    format!(
        "{}_{}_{}",
        sanitize_segment(prefix, "script"),
        Uuid::new_v4().simple(),
        &sha256[..12]
    )
}

fn ensure_within_root(path: &Path, root: &Path) -> ScriptMaterializationResult<()> {
    let root = canonicalize_existing_or_parent(root)?;
    let path = canonicalize_existing_or_parent(path)?;
    if !path.starts_with(&root) {
        return Err(ScriptMaterializationError::PathEscape(
            path.to_string_lossy().into_owned(),
        ));
    }
    Ok(())
}

fn canonicalize_existing_or_parent(path: &Path) -> io::Result<PathBuf> {
    if path.exists() {
        return path.canonicalize();
    }
    if let Some(parent) = path.parent()
        && parent.exists()
    {
        return Ok(parent
            .canonicalize()?
            .join(path.file_name().unwrap_or_default()));
    }
    Ok(path.to_path_buf())
}

fn metadata_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "script".to_string());
    path.with_file_name(format!("{name}.meta.json"))
}

fn read_descriptor(path: &Path) -> ScriptMaterializationResult<Option<MaterializedScript>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn read_descriptor_lenient(path: &Path) -> ScriptMaterializationResult<Option<MaterializedScript>> {
    match read_descriptor(path) {
        Ok(descriptor) => Ok(descriptor),
        Err(ScriptMaterializationError::Serde(_)) => Ok(None),
        Err(err) => Err(err),
    }
}

fn expires_at_from_ttl(ttl_secs: Option<u64>) -> Option<DateTime<Utc>> {
    let ttl = i64::try_from(ttl_secs?).ok()?;
    Utc::now().checked_add_signed(Duration::seconds(ttl))
}

fn file_uri_from_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

fn mime_for_language(language: &str) -> &'static str {
    match language {
        "python" | "py" => "text/x-python",
        "mel" => "text/x-mel",
        "javascript" | "js" => "text/javascript",
        "powershell" | "ps1" => "text/x-powershell",
        _ => "text/plain",
    }
}

fn cleanup_materialized_scripts(
    root: &Path,
    now: DateTime<Utc>,
    include_unexpired: bool,
) -> ScriptMaterializationResult<usize> {
    if !root.exists() {
        return Ok(0);
    }
    let root = root.canonicalize()?;
    let mut removed = 0;
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("json")
                || !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".meta.json"))
            {
                continue;
            }
            let Some(descriptor) = read_descriptor_lenient(&path)? else {
                continue;
            };
            if !include_unexpired && !descriptor.is_expired_at(now) {
                continue;
            }
            if remove_file_or_symlink_under_root(&descriptor.file_path, &root)? {
                removed += 1;
            }
            if remove_file_or_symlink_under_root(&path, &root)? {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

fn remove_file_or_symlink_under_root(path: &Path, root: &Path) -> io::Result<bool> {
    let candidate = canonicalize_parent_join(path)?;
    if !candidate.starts_with(root) {
        return Ok(false);
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let file_type = metadata.file_type();
    if file_type.is_file() || file_type.is_symlink() {
        fs::remove_file(path)?;
        return Ok(true);
    }
    Ok(false)
}

fn canonicalize_parent_join(path: &Path) -> io::Result<PathBuf> {
    if let Some(parent) = path.parent()
        && parent.exists()
    {
        return Ok(parent
            .canonicalize()?
            .join(path.file_name().unwrap_or_default()));
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration as StdDuration;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn sanitizes_path_segments_and_windows_style_values() {
        assert_eq!(sanitize_segment("maya/../../prod", "x"), "maya_.._.._prod");
        assert_eq!(
            sanitize_segment("C:\\show\\maya:2026", "x"),
            "C__show_maya_2026"
        );
        assert_eq!(sanitize_segment("...", "fallback"), "fallback");
    }

    #[test]
    fn materializes_descriptor_with_file_ref_metadata() {
        let tmp = tempdir().unwrap();
        let store = ScriptMaterializationStore::new_in(tmp.path());
        let mut request =
            ScriptMaterializationRequest::new("print('hello')", "maya", "inst-1", "sess-1");
        request.tool_call_id = Some("tool-1".to_string());
        request.correlation_id = Some("corr-1".to_string());
        request.ttl_secs = Some(60);

        let descriptor = store.materialize(request).unwrap();

        assert!(descriptor.file_path.is_file());
        assert_eq!(descriptor.bytes, 14);
        assert_eq!(descriptor.file_ref.session_id.as_deref(), Some("sess-1"));
        assert_eq!(descriptor.file_ref.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(
            descriptor.file_ref.digest,
            Some(format!("sha256:{}", descriptor.sha256))
        );
        assert_eq!(descriptor.file_ref.metadata["dcc_type"], "maya");
        assert!(descriptor.file_ref.uri.starts_with("file://"));
    }

    #[test]
    fn reuses_identical_content_when_requested() {
        let tmp = tempdir().unwrap();
        let store = ScriptMaterializationStore::new_in(tmp.path());
        let mut request = ScriptMaterializationRequest::new("x = 1", "custom", "i", "s");
        request.reuse = true;
        request.reuse_key = Some("bootstrap".to_string());

        let first = store.materialize(request.clone()).unwrap();
        let second = store.materialize(request).unwrap();

        assert_eq!(first.file_path, second.file_path);
        assert!(!first.reused);
        assert!(second.reused);
    }

    #[test]
    fn cleanup_removes_expired_scripts_only() {
        let tmp = tempdir().unwrap();
        let store = ScriptMaterializationStore::new_in(tmp.path());
        let mut expiring = ScriptMaterializationRequest::new("x = 1", "maya", "i", "s");
        expiring.ttl_secs = Some(1);
        let expired = store.materialize(expiring).unwrap();
        let live = store
            .materialize(ScriptMaterializationRequest::new("x = 2", "maya", "i", "s"))
            .unwrap();

        thread::sleep(StdDuration::from_secs(2));
        let removed = store.cleanup_expired(Utc::now()).unwrap();

        assert!(removed >= 2);
        assert!(!expired.file_path.exists());
        assert!(live.file_path.exists());
    }
}
