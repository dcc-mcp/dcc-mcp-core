//! Artefact hand-off primitive for the DCC-MCP ecosystem (issue #349).
//!
//! A workflow step (or any tool) often produces a *file* that a later step
//! needs to read — an imported scene, a QC report, an FBX export, a staged
//! `.uasset`. Passing the raw bytes inline bloats the transport; passing an
//! absolute path breaks across machines and can't be validated by clients.
//!
//! This crate introduces:
//!
//! - [`FileRef`] — a small, serializable value object that references a file
//!   by URI, carrying MIME / size / digest metadata.
//! - [`ArtefactStore`] — a trait that stores and retrieves artefact bodies
//!   keyed by URI, with content-addressed hashing (SHA-256) so duplicate
//!   bytes collapse to the same URI.
//! - [`FilesystemArtefactStore`] — default persistent backend that writes
//!   each artefact as `<workspace>/.dcc-mcp/artefacts/<sha256>.bin` with a
//!   JSON sidecar carrying the `FileRef` metadata.
//! - [`InMemoryArtefactStore`] — for tests and short-lived processes.
//!
//! The `artefact://` URI scheme is wired into the MCP Resources primitive
//! (issue #350) by `dcc-mcp-http` so MCP clients can `resources/read` a
//! `FileRef` by its URI.
//!
//! # Quick start
//!
//! ```no_run
//! use dcc_mcp_artefact::{FilesystemArtefactStore, ArtefactStore, ArtefactBody};
//! let store = FilesystemArtefactStore::new_in(std::env::temp_dir().join("artefacts"))
//!     .expect("open store");
//! let file_ref = store.put(ArtefactBody::Inline(b"hello".to_vec())).unwrap();
//! println!("stored as {}", file_ref.uri);
//! // Later: fetch it back.
//! let body = store.get(&file_ref.uri).unwrap().unwrap();
//! assert_eq!(body.into_bytes().unwrap(), b"hello");
//! ```

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[cfg(feature = "python-bindings")]
pub mod python;

/// Reference to a stored artefact.
///
/// The canonical form uses the `artefact://` scheme, e.g.
/// `artefact://sha256/<hex>`. Direct filesystem paths may still be surfaced
/// as `file:///absolute/path` when the producer opts out of content-
/// addressed storage (rare — reserve for huge pre-existing files).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileRef {
    /// Canonical URI for the artefact, e.g.
    /// `artefact://sha256/<hex>` or `file:///absolute/path`.
    pub uri: String,

    /// Optional MIME type (e.g. `image/png`, `application/json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,

    /// Size of the artefact body in bytes, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,

    /// Optional display filename/name for clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Canonical digest string, e.g. `sha256:<hex>`. Present for all
    /// artefacts stored via [`ArtefactStore::put`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,

    /// Job UUID that produced this artefact, when known. Used by
    /// [`ArtefactFilter::producer_job_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_job_id: Option<Uuid>,

    /// Tool call/request id that produced this artefact, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Session id that produced this artefact, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Adapter-defined correlation id for tracing across systems.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,

    /// Wall-clock time at which the artefact was registered.
    pub created_at: DateTime<Utc>,

    /// Wall-clock expiry time. Expired artefacts are hidden and cleaned up
    /// by bounded stores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,

    /// Tool-defined metadata (width/height for images, frame for captures,
    /// etc.). Never used by the store itself.
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl FileRef {
    /// Build a `FileRef` with only a URI; useful for tests and for wrapping
    /// external `file:///` references.
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            mime: None,
            size_bytes: None,
            display_name: None,
            digest: None,
            producer_job_id: None,
            tool_call_id: None,
            session_id: None,
            correlation_id: None,
            created_at: Utc::now(),
            expires_at: None,
            metadata: serde_json::Value::Null,
        }
    }

    /// Fluent setter for [`Self::mime`].
    pub fn with_mime(mut self, mime: impl Into<String>) -> Self {
        self.mime = Some(mime.into());
        self
    }

    /// Fluent setter for [`Self::producer_job_id`].
    pub fn with_producer_job_id(mut self, id: Uuid) -> Self {
        self.producer_job_id = Some(id);
        self
    }

    /// Fluent setter for [`Self::tool_call_id`].
    pub fn with_tool_call_id(mut self, id: impl Into<String>) -> Self {
        self.tool_call_id = Some(id.into());
        self
    }

    /// Fluent setter for [`Self::session_id`].
    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Fluent setter for [`Self::correlation_id`].
    pub fn with_correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    /// Fluent setter for [`Self::metadata`].
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Returns the hex digest portion if `digest` is of the form
    /// `sha256:<hex>`.
    pub fn sha256_hex(&self) -> Option<&str> {
        self.digest
            .as_deref()
            .and_then(|d| d.strip_prefix("sha256:"))
    }

    /// Return true when the artefact is past its expiry time.
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
    }
}

/// Body passed into [`ArtefactStore::put`].
///
/// `Stream` is intentionally *not* provided here — the two shapes we ship
/// cover the working set without pulling in `std::io` trait objects across
/// the FFI boundary. Callers that already have a `Read` should drain it into
/// a `Vec<u8>` first, or write it to a tempfile and use `Path`.
#[derive(Debug)]
pub enum ArtefactBody {
    /// In-memory buffer. Content-addressed stores will hash it before
    /// persisting.
    Inline(Vec<u8>),
    /// Path to a file already on disk. Content-addressed stores will stream-
    /// hash it and copy it to the canonical location; the source file is
    /// left untouched.
    Path(PathBuf),
}

impl ArtefactBody {
    /// Drain the body into a `Vec<u8>`.
    pub fn into_bytes(self) -> io::Result<Vec<u8>> {
        match self {
            ArtefactBody::Inline(bytes) => Ok(bytes),
            ArtefactBody::Path(p) => fs::read(p),
        }
    }
}

/// Filter passed to [`ArtefactStore::list`].
///
/// All fields are `Option`; `None` means "don't filter on this axis". An
/// empty filter returns every known artefact.
#[derive(Debug, Clone, Default)]
pub struct ArtefactFilter {
    pub producer_job_id: Option<Uuid>,
    pub mime: Option<String>,
    pub created_since: Option<DateTime<Utc>>,
    pub session_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub correlation_id: Option<String>,
}

/// Optional metadata and retention fields applied when storing an artefact.
#[derive(Debug, Clone)]
pub struct ArtefactPutOptions {
    pub mime: Option<String>,
    pub display_name: Option<String>,
    pub producer_job_id: Option<Uuid>,
    pub tool_call_id: Option<String>,
    pub session_id: Option<String>,
    pub correlation_id: Option<String>,
    pub ttl_secs: Option<u64>,
    pub metadata: serde_json::Value,
}

impl Default for ArtefactPutOptions {
    fn default() -> Self {
        Self {
            mime: None,
            display_name: None,
            producer_job_id: None,
            tool_call_id: None,
            session_id: None,
            correlation_id: None,
            ttl_secs: None,
            metadata: serde_json::Value::Null,
        }
    }
}

impl ArtefactPutOptions {
    fn apply_to(&self, file_ref: &mut FileRef, default_ttl_secs: Option<u64>) {
        file_ref.mime = self.mime.clone();
        file_ref.display_name = self.display_name.clone();
        file_ref.producer_job_id = self.producer_job_id;
        file_ref.tool_call_id = self.tool_call_id.clone();
        file_ref.session_id = self.session_id.clone();
        file_ref.correlation_id = self.correlation_id.clone();
        file_ref.metadata = self.metadata.clone();
        let ttl_secs = self.ttl_secs.or(default_ttl_secs);
        file_ref.expires_at = ttl_secs.and_then(expires_at_from_ttl);
    }
}

/// Configurable local artefact store bounds.
#[derive(Debug, Clone, Default)]
pub struct ArtefactStoreLimits {
    pub max_body_bytes: Option<u64>,
    pub max_entries: Option<usize>,
    pub max_total_bytes: Option<u64>,
    pub default_ttl_secs: Option<u64>,
}

/// Errors returned by [`ArtefactStore`] methods.
#[must_use]
#[derive(Debug, thiserror::Error)]
pub enum ArtefactError {
    #[error("artefact not found: {0}")]
    NotFound(String),
    #[error("invalid artefact URI: {0}")]
    InvalidUri(String),
    #[error("artefact exceeds configured limit: {0}")]
    LimitExceeded(String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type ArtefactResult<T> = Result<T, ArtefactError>;

/// Put/Get interface for storing artefacts keyed by content-addressed URI.
///
/// Implementations must be `Send + Sync` so that the MCP server can share a
/// single store between its Tokio workers and DCC main-thread callers.
pub trait ArtefactStore: Send + Sync {
    /// Store the body and return a freshly minted [`FileRef`].
    ///
    /// Content-addressed stores hash the body and deduplicate: submitting
    /// identical bytes returns the same URI.
    fn put(&self, body: ArtefactBody) -> ArtefactResult<FileRef>;

    /// Store the body with metadata/correlation/retention options.
    fn put_with_options(
        &self,
        body: ArtefactBody,
        options: ArtefactPutOptions,
    ) -> ArtefactResult<FileRef> {
        let mut file_ref = self.put(body)?;
        options.apply_to(&mut file_ref, None);
        Ok(file_ref)
    }

    /// Read the body for a previously-stored URI. Returns `Ok(None)` when
    /// the URI is unknown to this store (callers should treat that as
    /// `ResourceError::NotFound` at the MCP layer).
    fn get(&self, uri: &str) -> ArtefactResult<Option<ArtefactBody>>;

    /// Fetch just the metadata without opening the body. Useful for the
    /// MCP `resources/list` path.
    fn head(&self, uri: &str) -> ArtefactResult<Option<FileRef>>;

    /// Delete an artefact. Unknown URIs are a no-op (`Ok(())`).
    fn delete(&self, uri: &str) -> ArtefactResult<()>;

    /// Enumerate stored artefacts matching `filter`.
    fn list(&self, filter: ArtefactFilter) -> ArtefactResult<Vec<FileRef>>;
}

/// Build an `artefact://sha256/<hex>` URI.
pub fn make_uri_sha256(hex: &str) -> String {
    format!("artefact://sha256/{hex}")
}

/// Parse a URI of the form `artefact://sha256/<hex>` into the hex digest
/// component. Returns `None` for any other shape.
pub fn parse_sha256_uri(uri: &str) -> Option<&str> {
    uri.strip_prefix("artefact://sha256/")
}

/// Hash a byte slice with SHA-256 and return the lowercase hex digest.
pub fn hash_bytes_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Stream-hash a file with SHA-256 and return the lowercase hex digest.
pub fn hash_file_sha256(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn apply_filter(fr: &FileRef, filter: &ArtefactFilter) -> bool {
    if let Some(job) = filter.producer_job_id
        && fr.producer_job_id != Some(job)
    {
        return false;
    }
    if let Some(ref mime) = filter.mime
        && fr.mime.as_deref() != Some(mime.as_str())
    {
        return false;
    }
    if let Some(since) = filter.created_since
        && fr.created_at < since
    {
        return false;
    }
    if let Some(ref session_id) = filter.session_id
        && fr.session_id.as_deref() != Some(session_id.as_str())
    {
        return false;
    }
    if let Some(ref tool_call_id) = filter.tool_call_id
        && fr.tool_call_id.as_deref() != Some(tool_call_id.as_str())
    {
        return false;
    }
    if let Some(ref correlation_id) = filter.correlation_id
        && fr.correlation_id.as_deref() != Some(correlation_id.as_str())
    {
        return false;
    }
    true
}

fn expires_at_from_ttl(ttl_secs: u64) -> Option<DateTime<Utc>> {
    let ttl = i64::try_from(ttl_secs).ok()?;
    Utc::now().checked_add_signed(Duration::seconds(ttl))
}

// ── Filesystem backend ────────────────────────────────────────────────────

/// Persistent content-addressed store on local disk.
///
/// Layout (inside the configured root):
///
/// ```text
/// <root>/
///   <hex>.bin    # raw body
///   <hex>.json   # serialized FileRef sidecar
/// ```
///
/// Duplicate content (same SHA-256) reuses the existing files and returns
/// the existing sidecar's `FileRef`. Metadata supplied by the caller is
/// ignored for duplicates — first writer wins.
pub struct FilesystemArtefactStore {
    root: PathBuf,
    limits: ArtefactStoreLimits,
}

impl FilesystemArtefactStore {
    /// Create or open a store rooted at `path`. The directory is created
    /// recursively if it does not exist.
    pub fn new_in(path: impl Into<PathBuf>) -> io::Result<Self> {
        let root = path.into();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            limits: ArtefactStoreLimits::default(),
        })
    }

    /// Create or open a store with retention/size bounds.
    pub fn new_bounded_in(
        path: impl Into<PathBuf>,
        limits: ArtefactStoreLimits,
    ) -> io::Result<Self> {
        let root = path.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root, limits })
    }

    /// Root directory this store persists to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn body_path(&self, hex: &str) -> PathBuf {
        self.root.join(format!("{hex}.bin"))
    }

    fn sidecar_path(&self, hex: &str) -> PathBuf {
        self.root.join(format!("{hex}.json"))
    }

    fn write_sidecar(&self, file_ref: &FileRef, hex: &str) -> ArtefactResult<()> {
        let path = self.sidecar_path(hex);
        let tmp = path.with_extension("json.tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            let bytes = serde_json::to_vec(file_ref)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        fs::rename(tmp, path)?;
        Ok(())
    }

    fn read_sidecar(&self, hex: &str) -> ArtefactResult<Option<FileRef>> {
        let path = self.sidecar_path(hex);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn read_all_sidecars(&self) -> ArtefactResult<Vec<FileRef>> {
        let mut out = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            if let Ok(fr) = serde_json::from_slice::<FileRef>(&bytes) {
                out.push(fr);
            }
        }
        Ok(out)
    }

    fn cleanup_expired(&self) -> ArtefactResult<()> {
        let now = Utc::now();
        for fr in self.read_all_sidecars()? {
            if fr.is_expired_at(now) {
                self.delete(&fr.uri)?;
            }
        }
        Ok(())
    }

    fn enforce_retention_limits(&self) -> ArtefactResult<()> {
        self.cleanup_expired()?;
        let mut refs = self.read_all_sidecars()?;
        refs.sort_by_key(|fr| fr.created_at);
        let mut total_bytes: u64 = refs.iter().filter_map(|fr| fr.size_bytes).sum();
        while self
            .limits
            .max_entries
            .is_some_and(|max_entries| refs.len() > max_entries)
            || self
                .limits
                .max_total_bytes
                .is_some_and(|max_total_bytes| total_bytes > max_total_bytes)
        {
            let Some(fr) = refs.first().cloned() else {
                break;
            };
            total_bytes = total_bytes.saturating_sub(fr.size_bytes.unwrap_or(0));
            self.delete(&fr.uri)?;
            refs.remove(0);
        }
        Ok(())
    }

    fn put_inner(
        &self,
        body: ArtefactBody,
        options: ArtefactPutOptions,
    ) -> ArtefactResult<FileRef> {
        self.cleanup_expired()?;
        let (bytes, size) = match body {
            ArtefactBody::Inline(b) => {
                let len = b.len() as u64;
                (b, len)
            }
            ArtefactBody::Path(p) => {
                let b = fs::read(&p)?;
                let len = b.len() as u64;
                (b, len)
            }
        };
        if self
            .limits
            .max_body_bytes
            .is_some_and(|max_body_bytes| size > max_body_bytes)
        {
            return Err(ArtefactError::LimitExceeded(format!(
                "{size} bytes exceeds max_body_bytes={}",
                self.limits.max_body_bytes.unwrap_or_default()
            )));
        }
        let hex = hash_bytes_sha256(&bytes);
        let uri = make_uri_sha256(&hex);

        if let Some(existing) = self.read_sidecar(&hex)? {
            if existing.is_expired_at(Utc::now()) {
                self.delete(&existing.uri)?;
            } else {
                return Ok(existing);
            }
        }

        let body_path = self.body_path(&hex);
        if !body_path.exists() {
            let tmp = body_path.with_extension("bin.tmp");
            {
                let mut f = fs::File::create(&tmp)?;
                f.write_all(&bytes)?;
                f.sync_all()?;
            }
            fs::rename(&tmp, &body_path)?;
        }

        let mut file_ref = FileRef {
            uri: uri.clone(),
            mime: None,
            size_bytes: Some(size),
            display_name: None,
            digest: Some(format!("sha256:{hex}")),
            producer_job_id: None,
            tool_call_id: None,
            session_id: None,
            correlation_id: None,
            created_at: Utc::now(),
            expires_at: None,
            metadata: serde_json::Value::Null,
        };
        options.apply_to(&mut file_ref, self.limits.default_ttl_secs);
        self.write_sidecar(&file_ref, &hex)?;
        self.enforce_retention_limits()?;
        Ok(file_ref)
    }
}

impl ArtefactStore for FilesystemArtefactStore {
    fn put(&self, body: ArtefactBody) -> ArtefactResult<FileRef> {
        self.put_inner(body, ArtefactPutOptions::default())
    }

    fn put_with_options(
        &self,
        body: ArtefactBody,
        options: ArtefactPutOptions,
    ) -> ArtefactResult<FileRef> {
        self.put_inner(body, options)
    }

    fn get(&self, uri: &str) -> ArtefactResult<Option<ArtefactBody>> {
        let Some(hex) = parse_sha256_uri(uri) else {
            return Err(ArtefactError::InvalidUri(uri.to_string()));
        };
        if let Some(head) = self.read_sidecar(hex)?
            && head.is_expired_at(Utc::now())
        {
            self.delete(uri)?;
            return Ok(None);
        }
        let path = self.body_path(hex);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(ArtefactBody::Inline(bytes))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn head(&self, uri: &str) -> ArtefactResult<Option<FileRef>> {
        let Some(hex) = parse_sha256_uri(uri) else {
            return Err(ArtefactError::InvalidUri(uri.to_string()));
        };
        let head = self.read_sidecar(hex)?;
        if let Some(fr) = head
            && fr.is_expired_at(Utc::now())
        {
            self.delete(uri)?;
            return Ok(None);
        }
        self.read_sidecar(hex)
    }

    fn delete(&self, uri: &str) -> ArtefactResult<()> {
        let Some(hex) = parse_sha256_uri(uri) else {
            return Err(ArtefactError::InvalidUri(uri.to_string()));
        };
        let _ = fs::remove_file(self.body_path(hex));
        let _ = fs::remove_file(self.sidecar_path(hex));
        Ok(())
    }

    fn list(&self, filter: ArtefactFilter) -> ArtefactResult<Vec<FileRef>> {
        self.cleanup_expired()?;
        Ok(self
            .read_all_sidecars()?
            .into_iter()
            .filter(|fr| apply_filter(fr, &filter))
            .collect())
    }
}

// ── In-memory backend ─────────────────────────────────────────────────────

/// Non-persistent store keyed in memory. Useful for tests and for transient
/// CI runs where the FS backend would be overkill.
#[derive(Default)]
pub struct InMemoryArtefactStore {
    inner: RwLock<HashMap<String, (FileRef, Vec<u8>)>>,
    limits: ArtefactStoreLimits,
}

impl InMemoryArtefactStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limits(limits: ArtefactStoreLimits) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            limits,
        }
    }

    fn cleanup_expired_locked(&self, map: &mut HashMap<String, (FileRef, Vec<u8>)>) {
        let now = Utc::now();
        map.retain(|_, (fr, _)| !fr.is_expired_at(now));
    }

    fn enforce_retention_locked(&self, map: &mut HashMap<String, (FileRef, Vec<u8>)>) {
        self.cleanup_expired_locked(map);
        let mut refs: Vec<FileRef> = map.values().map(|(fr, _)| fr.clone()).collect();
        refs.sort_by_key(|fr| fr.created_at);
        let mut total_bytes: u64 = refs.iter().filter_map(|fr| fr.size_bytes).sum();
        while self
            .limits
            .max_entries
            .is_some_and(|max_entries| refs.len() > max_entries)
            || self
                .limits
                .max_total_bytes
                .is_some_and(|max_total_bytes| total_bytes > max_total_bytes)
        {
            let Some(fr) = refs.first().cloned() else {
                break;
            };
            total_bytes = total_bytes.saturating_sub(fr.size_bytes.unwrap_or(0));
            map.remove(&fr.uri);
            refs.remove(0);
        }
    }
}

impl ArtefactStore for InMemoryArtefactStore {
    fn put(&self, body: ArtefactBody) -> ArtefactResult<FileRef> {
        self.put_with_options(body, ArtefactPutOptions::default())
    }

    fn put_with_options(
        &self,
        body: ArtefactBody,
        options: ArtefactPutOptions,
    ) -> ArtefactResult<FileRef> {
        let bytes = body.into_bytes()?;
        let size = bytes.len() as u64;
        if self
            .limits
            .max_body_bytes
            .is_some_and(|max_body_bytes| size > max_body_bytes)
        {
            return Err(ArtefactError::LimitExceeded(format!(
                "{size} bytes exceeds max_body_bytes={}",
                self.limits.max_body_bytes.unwrap_or_default()
            )));
        }
        let hex = hash_bytes_sha256(&bytes);
        let uri = make_uri_sha256(&hex);
        {
            let mut map = self.inner.write();
            self.cleanup_expired_locked(&mut map);
            if let Some((existing, _)) = map.get(&uri) {
                return Ok(existing.clone());
            }
        }
        let mut fr = FileRef {
            uri: uri.clone(),
            mime: None,
            size_bytes: Some(size),
            display_name: None,
            digest: Some(format!("sha256:{hex}")),
            producer_job_id: None,
            tool_call_id: None,
            session_id: None,
            correlation_id: None,
            created_at: Utc::now(),
            expires_at: None,
            metadata: serde_json::Value::Null,
        };
        options.apply_to(&mut fr, self.limits.default_ttl_secs);
        let mut map = self.inner.write();
        map.insert(uri, (fr.clone(), bytes));
        self.enforce_retention_locked(&mut map);
        Ok(fr)
    }

    fn get(&self, uri: &str) -> ArtefactResult<Option<ArtefactBody>> {
        let mut map = self.inner.write();
        self.cleanup_expired_locked(&mut map);
        Ok(map
            .get(uri)
            .map(|(_, bytes)| ArtefactBody::Inline(bytes.clone())))
    }

    fn head(&self, uri: &str) -> ArtefactResult<Option<FileRef>> {
        let mut map = self.inner.write();
        self.cleanup_expired_locked(&mut map);
        Ok(map.get(uri).map(|(fr, _)| fr.clone()))
    }

    fn delete(&self, uri: &str) -> ArtefactResult<()> {
        self.inner.write().remove(uri);
        Ok(())
    }

    fn list(&self, filter: ArtefactFilter) -> ArtefactResult<Vec<FileRef>> {
        let mut map = self.inner.write();
        self.cleanup_expired_locked(&mut map);
        Ok(map
            .values()
            .filter_map(|(fr, _)| {
                if apply_filter(fr, &filter) {
                    Some(fr.clone())
                } else {
                    None
                }
            })
            .collect())
    }
}

// ── Convenience helpers ───────────────────────────────────────────────────

/// Store the file at `path` with optional MIME tag, returning the resulting
/// [`FileRef`]. Thin wrapper around [`ArtefactStore::put`] for the common
/// case.
pub fn put_file(
    store: &dyn ArtefactStore,
    path: impl Into<PathBuf>,
    mime: Option<String>,
) -> ArtefactResult<FileRef> {
    store.put_with_options(
        ArtefactBody::Path(path.into()),
        ArtefactPutOptions {
            mime,
            ..ArtefactPutOptions::default()
        },
    )
}

/// Store an in-memory buffer with optional MIME tag.
pub fn put_bytes(
    store: &dyn ArtefactStore,
    bytes: Vec<u8>,
    mime: Option<String>,
) -> ArtefactResult<FileRef> {
    store.put_with_options(
        ArtefactBody::Inline(bytes),
        ArtefactPutOptions {
            mime,
            ..ArtefactPutOptions::default()
        },
    )
}

/// Store the file at `path` with full metadata options.
pub fn put_file_with_options(
    store: &dyn ArtefactStore,
    path: impl Into<PathBuf>,
    options: ArtefactPutOptions,
) -> ArtefactResult<FileRef> {
    store.put_with_options(ArtefactBody::Path(path.into()), options)
}

/// Store an in-memory buffer with full metadata options.
pub fn put_bytes_with_options(
    store: &dyn ArtefactStore,
    bytes: Vec<u8>,
    options: ArtefactPutOptions,
) -> ArtefactResult<FileRef> {
    store.put_with_options(ArtefactBody::Inline(bytes), options)
}

/// Resolve an `artefact://` URI against `store`, returning either the
/// in-memory bytes or the on-disk path (when the store is filesystem-backed
/// and exposes the body file directly).
pub fn resolve(store: &dyn ArtefactStore, uri: &str) -> ArtefactResult<Option<ArtefactBody>> {
    store.get(uri)
}

/// Thread-safe wrapper type alias to pass stores around in `Arc`s.
pub type SharedArtefactStore = Arc<dyn ArtefactStore>;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
