use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Deserialize;

use super::types::{ProducerContent, ResourceError, ResourceProducer, ResourceResult};
use crate::protocol::McpResource;

#[derive(Default)]
pub(crate) struct SkillResourceState {
    loaded_skills: HashSet<String>,
    entries: BTreeMap<String, SkillResourceEntry>,
}

#[derive(Clone)]
struct SkillResourceEntry {
    resource: McpResource,
    yaml_dir: PathBuf,
    source: ResourceSourceSpec,
}

pub(crate) struct SkillResourceProducer {
    state: Arc<RwLock<SkillResourceState>>,
}

impl SkillResourceProducer {
    pub(crate) fn new(state: Arc<RwLock<SkillResourceState>>) -> Self {
        Self { state }
    }
}

impl ResourceProducer for SkillResourceProducer {
    fn scheme(&self) -> &str {
        "skill-resource"
    }

    fn list(&self) -> Vec<McpResource> {
        self.state
            .read()
            .entries
            .values()
            .map(|entry| entry.resource.clone())
            .collect()
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        let entry = self
            .state
            .read()
            .entries
            .get(uri)
            .cloned()
            .ok_or_else(|| ResourceError::NotFound(uri.to_string()))?;
        entry.read()
    }
}

impl SkillResourceEntry {
    fn read(&self) -> ResourceResult<ProducerContent> {
        let source_path = self.yaml_dir.join(&self.source.path);
        let bytes = std::fs::read(&source_path).map_err(|err| {
            ResourceError::Read(format!("failed to read {}: {err}", source_path.display()))
        })?;
        let mime_type = self
            .resource
            .mime_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string());
        if is_text_mime(&mime_type) {
            let text = String::from_utf8(bytes).map_err(|err| {
                ResourceError::Read(format!(
                    "{} is not valid UTF-8: {err}",
                    source_path.display()
                ))
            })?;
            Ok(ProducerContent::Text {
                uri: self.resource.uri.clone(),
                mime_type,
                text,
            })
        } else {
            Ok(ProducerContent::Blob {
                uri: self.resource.uri.clone(),
                mime_type,
                bytes,
            })
        }
    }
}

pub(crate) fn read_skill_resource(
    state: &Arc<RwLock<SkillResourceState>>,
    uri: &str,
) -> Option<ResourceResult<ProducerContent>> {
    let entry = state.read().entries.get(uri).cloned()?;
    Some(entry.read())
}

pub(crate) fn sync_skill_resources<F>(state: &Arc<RwLock<SkillResourceState>>, mut walk_loaded: F)
where
    F: FnMut(&mut dyn FnMut(&dcc_mcp_models::SkillMetadata)),
{
    let mut loaded_now = HashSet::new();
    let mut metadatas = Vec::new();
    let mut visit = |md: &dcc_mcp_models::SkillMetadata| {
        loaded_now.insert(md.name.clone());
        metadatas.push(md.clone());
    };
    walk_loaded(&mut visit);

    {
        let current = state.read();
        if current.loaded_skills == loaded_now {
            return;
        }
    }

    let mut entries = BTreeMap::new();
    for md in &metadatas {
        let Some(reference) = resources_reference(md) else {
            continue;
        };
        let skill_root = PathBuf::from(&md.skill_path);
        for entry in load_resources_from_reference(&skill_root, reference) {
            entries.insert(entry.resource.uri.clone(), entry);
        }
    }

    let mut current = state.write();
    current.loaded_skills = loaded_now;
    current.entries = entries;
}

fn resources_reference(md: &dcc_mcp_models::SkillMetadata) -> Option<&str> {
    md.metadata
        .pointer("/dcc-mcp/resources")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
}

fn load_resources_from_reference(skill_root: &Path, reference: &str) -> Vec<SkillResourceEntry> {
    let path = skill_root.join(reference);
    if path.is_dir() {
        return load_resource_dir(&path);
    }

    if reference.contains('*') || reference.contains('?') {
        let pattern_root = match reference.split_once('/') {
            Some((dir, _)) if !dir.contains('*') && !dir.contains('?') => skill_root.join(dir),
            _ => skill_root.to_path_buf(),
        };
        return load_resource_dir(&pattern_root);
    }

    load_resource_file(&path)
}

fn load_resource_dir(path: &Path) -> Vec<SkillResourceEntry> {
    let Ok(read_dir) = std::fs::read_dir(path) else {
        tracing::warn!(
            "resource sidecar directory {} missing or unreadable",
            path.display()
        );
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext, "yaml" | "yml"))
            .unwrap_or(false)
        {
            out.extend(load_resource_file(&path));
        }
    }
    out
}

fn load_resource_file(path: &Path) -> Vec<SkillResourceEntry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        tracing::warn!(
            "resource sidecar file {} missing or unreadable",
            path.display()
        );
        return Vec::new();
    };
    let specs = match serde_yaml_ng::from_str::<ResourceDocument>(&text) {
        Ok(doc) => doc.into_specs(),
        Err(err) => {
            tracing::warn!("failed to parse resource sidecar {}: {err}", path.display());
            return Vec::new();
        }
    };
    let yaml_dir = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    specs
        .into_iter()
        .filter_map(|spec| spec.into_entry(&yaml_dir))
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ResourceDocument {
    Wrapped { resources: Vec<ResourceSpec> },
    List(Vec<ResourceSpec>),
    Single(ResourceSpec),
}

impl ResourceDocument {
    fn into_specs(self) -> Vec<ResourceSpec> {
        match self {
            ResourceDocument::Wrapped { resources } | ResourceDocument::List(resources) => {
                resources
            }
            ResourceDocument::Single(resource) => vec![resource],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceSpec {
    uri: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    source: ResourceSourceSpec,
}

impl ResourceSpec {
    fn into_entry(self, yaml_dir: &Path) -> Option<SkillResourceEntry> {
        if self.uri.trim().is_empty() || self.name.trim().is_empty() {
            tracing::warn!("resource sidecar entry requires non-empty uri and name");
            return None;
        }
        if self.source.kind != "file" {
            tracing::warn!(
                "resource sidecar entry {} uses unsupported source.type {}; skipping",
                self.uri,
                self.source.kind
            );
            return None;
        }
        if self.source.path.trim().is_empty() {
            tracing::warn!("resource sidecar entry {} requires source.path", self.uri);
            return None;
        }
        Some(SkillResourceEntry {
            resource: McpResource {
                uri: self.uri,
                name: self.name,
                description: self.description,
                mime_type: self.mime_type,
            },
            yaml_dir: yaml_dir.to_path_buf(),
            source: self.source,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ResourceSourceSpec {
    #[serde(rename = "type")]
    kind: String,
    path: String,
}

fn is_text_mime(mime_type: &str) -> bool {
    mime_type.starts_with("text/")
        || matches!(
            mime_type,
            "application/json"
                | "application/yaml"
                | "application/x-yaml"
                | "application/xml"
                | "application/javascript"
        )
}
