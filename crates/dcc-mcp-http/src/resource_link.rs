//! Artifact → MCP `resource_link` conversion (issue #243).
//!
//! On MCP **2025-06-18** sessions, tools/call results may include
//! `resource_link` content items that point to DCC-produced artifact files
//! (playblasts, FBX/USD exports, screenshots, baked textures).  Instead of
//! base64-encoding the bytes into `content[0].text` — catastrophic for token
//! cost — we hand the agent a URI it can resolve on demand.
//!
//! Tool authors surface artifacts by including one of the following keys in
//! their JSON payload:
//!
//! | Key              | Shape                                           |
//! |------------------|-------------------------------------------------|
//! | `artifact_paths` | `["/abs/out.mp4", ...]`                         |
//! | `artifacts`      | `[{"path": "...", "name": "...", "mime": "..."}]` |
//! | `artifact_path`  | `"/abs/out.mp4"` (single path, legacy)          |
//!
//! On MCP **2025-03-26** sessions we emit no `resource_link` items (the spec
//! did not define the type); the existing text fallback is preserved by the
//! caller.

use std::path::Path;

use serde_json::Value;

use crate::protocol::ToolContent;

/// Heuristic mapping from file extension to a common MIME type.
///
/// Intentionally small — specialised DCC adapters are free to supply their
/// own `mime` on `artifacts[].mime` when they know better.
fn guess_mime_type(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        // Video / playblast
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "webm" => "video/webm",
        // Images / screenshots
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "tif" | "tiff" => "image/tiff",
        "exr" => "image/x-exr",
        "webp" => "image/webp",
        // 3D / DCC exchange
        "fbx" => "application/octet-stream",
        "obj" => "model/obj",
        "usd" | "usda" | "usdc" | "usdz" => "model/vnd.usd",
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        "abc" => "application/x-alembic",
        // Textures / data
        "hdr" => "image/vnd.radiance",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" | "log" => "text/plain",
        _ => return None,
    })
}

/// Convert an absolute or relative filesystem path into a `file://` URI.
///
/// This is a best-effort conversion; it does not canonicalise the path (we
/// must not touch the filesystem here — the action may return paths to files
/// that live on the DCC host, not on the HTTP server).
fn path_to_file_uri(path: &str) -> String {
    if path.starts_with("file://") || path.contains("://") {
        return path.to_string();
    }
    // Windows: replace backslashes; absolute paths need a leading slash.
    let normalised = path.replace('\\', "/");
    if normalised.starts_with('/') {
        format!("file://{normalised}")
    } else if normalised.len() >= 2 && normalised.as_bytes()[1] == b':' {
        // e.g. "C:/foo/bar.mp4" → "file:///C:/foo/bar.mp4"
        format!("file:///{normalised}")
    } else {
        // Relative path — still emit file:// so the client can resolve against cwd.
        format!("file://{normalised}")
    }
}

/// Build a single `ResourceLink` from a filesystem path.
fn resource_link_from_path(path: &str, name: Option<&str>, mime: Option<&str>) -> ToolContent {
    let file_name = name.map(str::to_owned).or_else(|| {
        Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned)
    });
    let mime_type = mime
        .map(str::to_owned)
        .or_else(|| guess_mime_type(path).map(str::to_owned));
    ToolContent::ResourceLink {
        uri: path_to_file_uri(path),
        name: file_name,
        mime_type,
        description: None,
    }
}

/// Extract `resource_link` content items from a tool-output payload.
///
/// Recognised shapes (first match wins):
/// - `{"artifact_paths": ["/a.mp4", "/b.png"]}`
/// - `{"artifacts": [{"path": "/a.mp4", "name": "...", "mime": "..."}, ...]}`
/// - `{"artifact_path": "/a.mp4"}`
///
/// Returns an empty vec when no artifacts are present.
pub fn extract_resource_links(output: &Value) -> Vec<ToolContent> {
    let obj = match output.as_object() {
        Some(o) => o,
        None => return Vec::new(),
    };

    if let Some(arr) = obj.get("artifact_paths").and_then(Value::as_array) {
        return arr
            .iter()
            .filter_map(Value::as_str)
            .map(|p| resource_link_from_path(p, None, None))
            .collect();
    }

    if let Some(arr) = obj.get("artifacts").and_then(Value::as_array) {
        return arr
            .iter()
            .filter_map(|entry| {
                let e = entry.as_object()?;
                let path = e.get("path").and_then(Value::as_str)?;
                let name = e.get("name").and_then(Value::as_str);
                // Accept both `mime` (terse) and `mimeType` (MCP camelCase) / `mime_type`.
                let mime = e
                    .get("mime")
                    .and_then(Value::as_str)
                    .or_else(|| e.get("mimeType").and_then(Value::as_str))
                    .or_else(|| e.get("mime_type").and_then(Value::as_str));
                let description = e.get("description").and_then(Value::as_str);
                match resource_link_from_path(path, name, mime) {
                    ToolContent::ResourceLink {
                        uri,
                        name,
                        mime_type,
                        ..
                    } => Some(ToolContent::ResourceLink {
                        uri,
                        name,
                        mime_type,
                        description: description.map(str::to_owned),
                    }),
                    other => Some(other),
                }
            })
            .collect();
    }

    if let Some(path) = obj.get("artifact_path").and_then(Value::as_str) {
        return vec![resource_link_from_path(path, None, None)];
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn guesses_common_mime_types() {
        assert_eq!(guess_mime_type("/tmp/out.mp4"), Some("video/mp4"));
        assert_eq!(guess_mime_type("shot.PNG"), Some("image/png"));
        assert_eq!(guess_mime_type("scene.usda"), Some("model/vnd.usd"));
        assert_eq!(guess_mime_type("unknown.xyz"), None);
    }

    #[test]
    fn converts_unix_path_to_file_uri() {
        assert_eq!(path_to_file_uri("/tmp/a.mp4"), "file:///tmp/a.mp4");
    }

    #[test]
    fn converts_windows_path_to_file_uri() {
        assert_eq!(
            path_to_file_uri("C:\\work\\out.mp4"),
            "file:///C:/work/out.mp4"
        );
    }

    #[test]
    fn preserves_existing_uri() {
        assert_eq!(
            path_to_file_uri("https://cdn.example.com/out.mp4"),
            "https://cdn.example.com/out.mp4"
        );
        assert_eq!(
            path_to_file_uri("file:///already/uri.png"),
            "file:///already/uri.png"
        );
    }

    #[test]
    fn extracts_artifact_paths_array() {
        let out = json!({
            "artifact_paths": ["/tmp/a.mp4", "/tmp/b.png"],
            "frame_count": 24,
        });
        let links = extract_resource_links(&out);
        assert_eq!(links.len(), 2);
        match &links[0] {
            ToolContent::ResourceLink {
                uri,
                mime_type,
                name,
                ..
            } => {
                assert_eq!(uri, "file:///tmp/a.mp4");
                assert_eq!(mime_type.as_deref(), Some("video/mp4"));
                assert_eq!(name.as_deref(), Some("a.mp4"));
            }
            _ => panic!("expected ResourceLink"),
        }
    }

    #[test]
    fn extracts_structured_artifacts() {
        let out = json!({
            "artifacts": [
                {
                    "path": "/tmp/anim.mov",
                    "name": "Shot 010 playblast",
                    "mime": "video/quicktime",
                    "description": "1280x720, 240 frames"
                }
            ]
        });
        let links = extract_resource_links(&out);
        assert_eq!(links.len(), 1);
        match &links[0] {
            ToolContent::ResourceLink {
                uri,
                name,
                mime_type,
                description,
            } => {
                assert_eq!(uri, "file:///tmp/anim.mov");
                assert_eq!(name.as_deref(), Some("Shot 010 playblast"));
                assert_eq!(mime_type.as_deref(), Some("video/quicktime"));
                assert_eq!(description.as_deref(), Some("1280x720, 240 frames"));
            }
            _ => panic!("expected ResourceLink"),
        }
    }

    #[test]
    fn extracts_single_artifact_path() {
        let out = json!({"artifact_path": "/var/out/bake.exr"});
        let links = extract_resource_links(&out);
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn returns_empty_when_no_artifacts() {
        let out = json!({"nodes": ["pSphere1"]});
        assert!(extract_resource_links(&out).is_empty());
    }

    #[test]
    fn accepts_mime_type_camel_and_snake() {
        let out = json!({
            "artifacts": [
                {"path": "/a.bin", "mimeType": "application/custom-a"},
                {"path": "/b.bin", "mime_type": "application/custom-b"}
            ]
        });
        let links = extract_resource_links(&out);
        assert_eq!(links.len(), 2);
        if let ToolContent::ResourceLink { mime_type, .. } = &links[0] {
            assert_eq!(mime_type.as_deref(), Some("application/custom-a"));
        }
        if let ToolContent::ResourceLink { mime_type, .. } = &links[1] {
            assert_eq!(mime_type.as_deref(), Some("application/custom-b"));
        }
    }

    #[test]
    fn serializes_with_resource_link_type_tag() {
        let link = ToolContent::ResourceLink {
            uri: "file:///tmp/a.mp4".into(),
            name: Some("a.mp4".into()),
            mime_type: Some("video/mp4".into()),
            description: None,
        };
        let v = serde_json::to_value(&link).unwrap();
        assert_eq!(v.get("type").and_then(Value::as_str), Some("resource_link"));
        assert_eq!(
            v.get("uri").and_then(Value::as_str),
            Some("file:///tmp/a.mp4")
        );
        assert_eq!(v.get("mimeType").and_then(Value::as_str), Some("video/mp4"));
        assert!(v.get("description").is_none());
    }
}
