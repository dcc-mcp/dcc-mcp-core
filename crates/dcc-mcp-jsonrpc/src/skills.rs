//! `io.modelcontextprotocol/skills` wire types (MCP 2026-07-28 extension).
//!
//! The extension publishes [Agent Skills](https://agentskills.io/) over the
//! existing Resources primitive: every file of a skill is an individually
//! addressable resource, discovered through `skills/list` / `skills/get` and
//! read through `resources/read`.
//!
//! This module holds only the wire shapes and their method names. Building a
//! manifest from a skill directory lives in `dcc-mcp-skills::skill_content`.
//!
//! Reference: <https://github.com/modelcontextprotocol/ext-skills/blob/main/specification/stable/skills.mdx>

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::resources::McpResource;

/// `skills/list` — enumerate the skills a server serves.
pub const SKILLS_LIST_METHOD: &str = "skills/list";

/// `skills/get` — return the entry for one skill by `SKILL.md` URI.
pub const SKILLS_GET_METHOD: &str = "skills/get";

/// `resources/directory/read` — list the direct children of a directory
/// resource. Gated behind the extension's `directoryRead` setting.
pub const RESOURCE_DIRECTORY_READ_METHOD: &str = "resources/directory/read";

/// Literal used in place of a file manifest when a skill's content is
/// generated and no stable digests can be published.
pub const DYNAMIC_RESOURCES: &str = "dynamic";

/// One file of a skill, with the digest and size of its content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillResource {
    /// Resource URI of the file.
    pub uri: String,
    /// SHA-256 digest of the raw bytes, formatted `sha256:{hex}`.
    pub digest: String,
    /// Length in bytes of the raw content the digest covers.
    pub size: u64,
}

/// A skill's files: a complete enumeration, or the `"dynamic"` marker.
///
/// `resources` is REQUIRED on every entry and takes one of exactly these two
/// forms; any other value makes the entry invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillResources {
    /// Complete enumeration of `SKILL.md` and every supporting file.
    Entries(Vec<SkillResource>),
    /// The skill's content is generated; no stable digests can be published.
    Dynamic,
}

impl Serialize for SkillResources {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Entries(entries) => entries.serialize(serializer),
            Self::Dynamic => serializer.serialize_str(DYNAMIC_RESOURCES),
        }
    }
}

impl<'de> Deserialize<'de> for SkillResources {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(marker) if marker == DYNAMIC_RESOURCES => Ok(Self::Dynamic),
            Value::String(other) => Err(serde::de::Error::custom(format!(
                "resources must be an array or `\"{DYNAMIC_RESOURCES}\"`, got `{other}`"
            ))),
            Value::Array(_) => serde_json::from_value(value)
                .map(Self::Entries)
                .map_err(|error: serde_json::Error| serde::de::Error::custom(error.to_string())),
            other => Err(serde::de::Error::custom(format!(
                "resources must be an array or `\"{DYNAMIC_RESOURCES}\"`, got `{other}`"
            ))),
        }
    }
}

/// The entry for a single skill, as returned by `skills/list` and `skills/get`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    /// Resource URI of the skill's `SKILL.md`, readable via `resources/read`.
    pub uri: String,
    /// The `SKILL.md` YAML frontmatter, rendered verbatim as JSON.
    pub frontmatter: Map<String, Value>,
    /// The skill's complete file manifest, or the dynamic marker.
    pub resources: SkillResources,
}

/// Result payload for `skills/list`.
///
/// Extends `PaginatedResult` and `CacheableResult`; `resultType`, `ttlMs` and
/// `cacheScope` are added at the modern response boundary by
/// [`crate::complete_modern_result`]. An entry is atomic — a skill's
/// `resources` set is never split across pages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSkillsResult {
    pub skills: Vec<Skill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Request params for `skills/get`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSkillParams {
    /// URI of the skill's `SKILL.md`.
    pub uri: String,
}

/// Result payload for `skills/get`.
///
/// Extends `CacheableResult`. Carries no pagination cursor: it is a
/// point-in-time snapshot of one skill's entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSkillResult {
    pub skill: Option<Skill>,
}

/// Request params for `resources/directory/read`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResourceDirectoryParams {
    /// URI of the directory resource to read, written without a trailing slash.
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Result payload for `resources/directory/read`.
///
/// Contains every direct child of the directory; subdirectories are listed as
/// directory resources (`mimeType: "inode/directory"`). The listing is not
/// recursive — clients descend by calling the method again.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResourceDirectoryResult {
    pub resources: Vec<McpResource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_skill() -> Skill {
        Skill {
            uri: "skill://pdf-processing/SKILL.md".to_string(),
            frontmatter: json!({
                "name": "pdf-processing",
                "description": "Extract, fill, and assemble PDF documents",
                "license": "Apache-2.0",
            })
            .as_object()
            .expect("object")
            .clone(),
            resources: SkillResources::Entries(vec![SkillResource {
                uri: "skill://pdf-processing/SKILL.md".to_string(),
                digest: "sha256:99b737495721155ece826d57521e2d66141ebdc1344a400487481ea2642ab19e"
                    .to_string(),
                size: 151,
            }]),
        }
    }

    #[test]
    fn skill_entry_serialises_to_the_specification_shape() {
        let value = serde_json::to_value(sample_skill()).unwrap();
        assert_eq!(value["uri"], "skill://pdf-processing/SKILL.md");
        // Frontmatter passes through verbatim, including unknown keys.
        assert_eq!(value["frontmatter"]["name"], "pdf-processing");
        assert_eq!(value["frontmatter"]["license"], "Apache-2.0");
        assert_eq!(value["resources"][0]["size"], 151);
        assert_eq!(
            value["resources"][0]["digest"],
            "sha256:99b737495721155ece826d57521e2d66141ebdc1344a400487481ea2642ab19e"
        );
    }

    #[test]
    fn dynamic_resources_round_trip_as_the_literal_marker() {
        let skill = Skill {
            resources: SkillResources::Dynamic,
            ..sample_skill()
        };
        let value = serde_json::to_value(&skill).unwrap();
        assert_eq!(value["resources"], json!("dynamic"));
        let recovered: Skill = serde_json::from_value(value).unwrap();
        assert_eq!(recovered.resources, SkillResources::Dynamic);
    }

    #[test]
    fn resources_reject_any_value_other_than_an_array_or_the_marker() {
        for invalid in [json!("partial"), json!(null), json!(42), json!({})] {
            let entry = json!({
                "uri": "skill://demo/SKILL.md",
                "frontmatter": {"name": "demo", "description": "d"},
                "resources": invalid,
            });
            let error = serde_json::from_value::<Skill>(entry).unwrap_err();
            assert!(
                error.to_string().contains("resources must be an array"),
                "unexpected error for {invalid}: {error}"
            );
        }
    }

    #[test]
    fn list_and_get_results_use_camel_case_cursors() {
        let list = ListSkillsResult {
            skills: vec![sample_skill()],
            next_cursor: Some("eyJvIjoxfQ".to_string()),
        };
        let value = serde_json::to_value(&list).unwrap();
        assert!(value.get("nextCursor").is_some());
        assert!(value.get("next_cursor").is_none());

        let get = GetSkillResult {
            skill: Some(sample_skill()),
        };
        let value = serde_json::to_value(&get).unwrap();
        assert_eq!(value["skill"]["uri"], "skill://pdf-processing/SKILL.md");

        let directory = ReadResourceDirectoryResult {
            resources: vec![McpResource {
                uri: "skill://pdf-processing/templates".to_string(),
                name: "templates".to_string(),
                description: None,
                mime_type: Some("inode/directory".to_string()),
            }],
            next_cursor: None,
        };
        let value = serde_json::to_value(&directory).unwrap();
        // An absent cursor is omitted, not null.
        assert!(value.get("nextCursor").is_none());
        assert_eq!(value["resources"][0]["mimeType"], "inode/directory");
    }
}
