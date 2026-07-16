use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use base64::Engine;
pub(super) use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::{Map, Value};

const MAX_INLINE_IMAGE_BASE64_BYTES: usize = 32 * 1024 * 1024;
const IMAGE_ARTIFACT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
const IMAGE_ARTIFACT_MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const IMAGE_ARTIFACT_SIZE_PRUNE_GRACE: Duration = Duration::from_secs(60);
pub(super) const MATERIALIZED_IMAGE_PLACEHOLDER: &str =
    "<omitted; materialized by dcc-mcp-cli; see artifact_path>";

pub(super) fn default_image_artifact_root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("dcc-mcp")
        .join("call-artifacts")
}

pub(super) fn materialize_call_images(value: &mut Value, root: &Path) {
    match value {
        Value::Object(object) => {
            if object.get("kind").and_then(Value::as_str) == Some("image") {
                materialize_image_payload(object, "mime", root);
            } else if object.get("type").and_then(Value::as_str) == Some("image") {
                let mime_key = if object.contains_key("mimeType") {
                    "mimeType"
                } else {
                    "mime_type"
                };
                materialize_image_payload(object, mime_key, root);
            }
            for child in object.values_mut() {
                materialize_call_images(child, root);
            }
        }
        Value::Array(items) => {
            for item in items {
                materialize_call_images(item, root);
            }
        }
        _ => {}
    }
}

fn materialize_image_payload(image: &mut Map<String, Value>, mime_key: &str, root: &Path) {
    let has_artifact = image
        .get("artifact_path")
        .and_then(Value::as_str)
        .is_some_and(|path| !path.trim().is_empty());
    let encoded = match image.remove("data") {
        Some(Value::String(encoded)) if encoded.starts_with("<omitted;") => {
            image.insert("data".to_string(), Value::String(encoded));
            return;
        }
        Some(Value::String(encoded)) => {
            image.insert(
                "data".to_string(),
                Value::String(MATERIALIZED_IMAGE_PLACEHOLDER.to_string()),
            );
            encoded
        }
        Some(_) => {
            image.insert(
                "data".to_string(),
                Value::String(MATERIALIZED_IMAGE_PLACEHOLDER.to_string()),
            );
            record_image_materialization_error(image, "image data must be a base64 string");
            return;
        }
        None if has_artifact => return,
        None => {
            record_image_materialization_error(image, "missing image data");
            return;
        }
    };

    let Some(extension) = image
        .get(mime_key)
        .and_then(Value::as_str)
        .and_then(image_extension)
    else {
        record_image_materialization_error(image, "unsupported or missing image MIME type");
        return;
    };
    if encoded.len() > MAX_INLINE_IMAGE_BASE64_BYTES {
        record_image_materialization_error(image, "inline image exceeds the 32 MiB base64 limit");
        return;
    }
    let bytes = match BASE64_STANDARD.decode(encoded.trim()) {
        Ok(bytes) => bytes,
        Err(_) => {
            record_image_materialization_error(image, "invalid base64 image data");
            return;
        }
    };
    if bytes.is_empty() {
        record_image_materialization_error(image, "decoded image data is empty");
        return;
    }
    image.remove("materialization_error");
    if has_artifact {
        return;
    }
    let path = match write_image_artifact(root, extension, &bytes) {
        Ok(path) => path,
        Err(err) => {
            record_image_materialization_error(
                image,
                &format!("failed to write image artifact: {err}"),
            );
            return;
        }
    };
    image.insert(
        "artifact_path".to_string(),
        Value::String(path.display().to_string()),
    );
}

fn image_extension(mime: &str) -> Option<&'static str> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

fn record_image_materialization_error(image: &mut Map<String, Value>, message: &str) {
    image.insert(
        "materialization_error".to_string(),
        Value::String(message.to_string()),
    );
}

fn write_image_artifact(root: &Path, extension: &str, bytes: &[u8]) -> anyhow::Result<PathBuf> {
    let root = std::path::absolute(root).context("resolving image artifact directory")?;
    std::fs::create_dir_all(&root)
        .with_context(|| format!("creating image artifact directory {}", root.display()))?;
    let suffix = format!(".{extension}");
    let mut file = tempfile::Builder::new()
        .prefix("computer-use-")
        .suffix(&suffix)
        .tempfile_in(&root)
        .context("creating unique image artifact")?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    let (_, path) = file
        .keep()
        .map_err(|err| err.error)
        .context("persisting image artifact")?;
    prune_image_artifacts(
        &root,
        std::time::SystemTime::now(),
        IMAGE_ARTIFACT_RETENTION,
        IMAGE_ARTIFACT_MAX_TOTAL_BYTES,
        Some(&path),
    );
    Ok(path)
}

pub(super) fn prune_image_artifacts(
    root: &Path,
    now: std::time::SystemTime,
    retention: Duration,
    max_total_bytes: u64,
    protected: Option<&Path>,
) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let cutoff = (!retention.is_zero())
        .then(|| now.checked_sub(retention))
        .flatten();
    let size_prune_cutoff = now.checked_sub(IMAGE_ARTIFACT_SIZE_PRUNE_GRACE);
    let mut total_size = 0_u64;
    let mut candidates = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if protected.is_some_and(|protected| path == protected)
            || !is_owned_image_artifact(&path)
            || !entry
                .file_type()
                .ok()
                .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
        if cutoff.is_some_and(|cutoff| modified < cutoff) {
            let _ = std::fs::remove_file(path);
            continue;
        }
        let size = metadata.len();
        total_size = total_size.saturating_add(size);
        candidates.push((modified, size, path));
    }

    if let Some(protected) = protected
        && let Ok(metadata) = std::fs::metadata(protected)
    {
        total_size = total_size.saturating_add(metadata.len());
    }
    if max_total_bytes == 0 || total_size <= max_total_bytes {
        return;
    }

    candidates.sort_by_key(|entry| entry.0);
    for (modified, size, path) in candidates {
        if total_size <= max_total_bytes {
            break;
        }
        if size_prune_cutoff.is_some_and(|cutoff| modified >= cutoff) {
            continue;
        }
        if std::fs::remove_file(path).is_ok() {
            total_size = total_size.saturating_sub(size);
        }
    }
}

fn is_owned_image_artifact(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("computer-use-"))
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "webp" | "gif"
                )
            })
}
