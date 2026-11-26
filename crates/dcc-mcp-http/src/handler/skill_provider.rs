//! Concrete provider for the MCP Skills extension.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use dcc_mcp_http_server::rmcp_providers::{ProviderError, SkillProvider};
use dcc_mcp_jsonrpc::{
    GetSkillResult, ListSkillsResult, McpResource, ReadResourceDirectoryResult, ReadResourceResult,
    ResourceContents, Skill, SkillResource, SkillResources,
};
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_skills::constants::SKILL_METADATA_FILE;
use dcc_mcp_skills::skill_content::{self, DIRECTORY_MIME_TYPE, SKILL_MD_MIME_TYPE, SkillFileSet};

/// Reads discovered skill directories without activating their tools.
#[derive(Debug, Default)]
pub struct SkillCatalogProvider;

impl SkillProvider for SkillCatalogProvider {
    fn list_skills(&self, catalog: &Arc<SkillCatalog>) -> ListSkillsResult {
        let mut skills = published_skills(catalog);
        skills.sort_by(|a, b| a.uri.cmp(&b.uri));
        ListSkillsResult {
            skills,
            next_cursor: None,
        }
    }

    fn get_skill(
        &self,
        uri: &str,
        catalog: &Arc<SkillCatalog>,
    ) -> Result<GetSkillResult, ProviderError> {
        let (skill_name, relative) = parse_skill_uri(uri)?;
        if relative != SKILL_METADATA_FILE {
            return Err(not_found(format!("{uri} is not the URI of a SKILL.md")));
        }
        let Some(directory) = publishable_skill_dir(catalog, skill_name)? else {
            return Err(not_found(format!("No skill is served at {skill_name}")));
        };
        let manifest =
            skill_content::build_skill_manifest(&directory, skill_name).map_err(|error| {
                not_found(format!("skill `{skill_name}` cannot be published: {error}"))
            })?;
        Ok(GetSkillResult {
            skill: Some(to_wire_skill(&manifest)),
        })
    }

    fn list_skill_resources(&self, catalog: &Arc<SkillCatalog>) -> Vec<McpResource> {
        let mut resources = published_skill_dirs(catalog)
            .into_iter()
            .filter_map(|(name, directory)| {
                let frontmatter = skill_content::check_publishable(&directory, &name).ok()?;
                Some(McpResource {
                    uri: skill_content::skill_uri(&name, SKILL_METADATA_FILE),
                    name: frontmatter.name().map(str::to_owned).unwrap_or(name),
                    description: frontmatter.description().map(str::to_owned),
                    mime_type: Some(SKILL_MD_MIME_TYPE.to_owned()),
                })
            })
            .collect::<Vec<_>>();
        resources.sort_by(|a, b| a.uri.cmp(&b.uri));
        resources
    }

    fn read_skill_resource(
        &self,
        uri: &str,
        catalog: &Arc<SkillCatalog>,
    ) -> Result<ReadResourceResult, ProviderError> {
        let (skill_name, relative) = parse_skill_uri(uri)?;
        let Some(directory) = publishable_skill_dir(catalog, skill_name)? else {
            return Err(not_found(format!("No skill is served at {skill_name}")));
        };
        if relative.is_empty() {
            return Err(not_found(format!(
                "{uri} is a directory resource; use resources/directory/read"
            )));
        }
        let Some(path) = skill_content::resolve_skill_file(&directory, relative) else {
            return Err(not_found(format!(
                "{uri} is not a file of skill `{skill_name}`"
            )));
        };
        if !path.is_file() {
            return Err(not_found(format!(
                "{uri} is not a file resource; use resources/directory/read"
            )));
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| ProviderError::Internal(format!("cannot read {uri}: {error}")))?;
        let mime_type = skill_content::mime_type_for(relative).to_owned();
        let contents = match String::from_utf8(bytes) {
            Ok(text) => ResourceContents {
                uri: uri.to_owned(),
                mime_type: Some(mime_type),
                text: Some(text),
                blob: None,
            },
            Err(error) => ResourceContents {
                uri: uri.to_owned(),
                mime_type: Some(mime_type),
                text: None,
                blob: Some(BASE64_STANDARD.encode(error.into_bytes())),
            },
        };
        Ok(ReadResourceResult {
            contents: vec![contents],
        })
    }

    fn read_skill_directory(
        &self,
        uri: &str,
        catalog: &Arc<SkillCatalog>,
    ) -> Result<ReadResourceDirectoryResult, ProviderError> {
        let (skill_name, relative) = parse_skill_uri(uri)?;
        let Some(directory_root) = publishable_skill_dir(catalog, skill_name)? else {
            return Err(not_found(format!("No skill is served at {skill_name}")));
        };
        let directory = if relative.is_empty() {
            directory_root.clone()
        } else {
            skill_content::resolve_skill_file(&directory_root, relative)
                .ok_or_else(|| not_found(format!("{uri} is not a directory resource")))?
        };
        if !directory.is_dir() {
            return Err(not_found(format!("{uri} is not a directory resource")));
        }
        let mut resources = directory_children(skill_name, &directory_root, &directory)?;
        resources.sort_by(|a, b| a.uri.cmp(&b.uri));
        Ok(ReadResourceDirectoryResult {
            resources,
            next_cursor: None,
        })
    }
}

fn published_skills(catalog: &Arc<SkillCatalog>) -> Vec<Skill> {
    published_skill_dirs(catalog)
        .into_iter()
        .filter_map(|(name, directory)| {
            match skill_content::build_skill_manifest(&directory, &name) {
                Ok(manifest) => Some(to_wire_skill(&manifest)),
                Err(error) => {
                    tracing::warn!(
                        skill = %name,
                        path = %directory.display(),
                        %error,
                        "skills: not publishing skill through the Skills extension"
                    );
                    None
                }
            }
        })
        .collect()
}

fn to_wire_skill(manifest: &dcc_mcp_skills::skill_content::SkillManifest) -> Skill {
    let resources = match &manifest.resources {
        SkillFileSet::Files(files) => SkillResources::Entries(
            files
                .iter()
                .map(|file| SkillResource {
                    uri: file.uri.clone(),
                    digest: file.digest.clone(),
                    size: file.size,
                })
                .collect(),
        ),
        SkillFileSet::Dynamic => SkillResources::Dynamic,
    };
    Skill {
        uri: manifest.uri.clone(),
        frontmatter: manifest.frontmatter.clone().into_object(),
        resources,
    }
}

fn parse_skill_uri(uri: &str) -> Result<(&str, &str), ProviderError> {
    skill_content::parse_skill_uri(uri)
        .ok_or_else(|| not_found(format!("{uri} is not a skill resource URI")))
}

fn published_skill_dirs(catalog: &Arc<SkillCatalog>) -> Vec<(String, PathBuf)> {
    let mut skills = catalog
        .list_skills(None)
        .into_iter()
        .filter_map(|summary| {
            catalog
                .get_skill(&summary.name)
                .map(|metadata| (summary.name, PathBuf::from(metadata.skill_path)))
        })
        .filter(|(_, directory)| !directory.as_os_str().is_empty())
        .collect::<Vec<_>>();
    skills.sort_by(|a, b| a.0.cmp(&b.0));
    skills
}

fn publishable_skill_dir(
    catalog: &Arc<SkillCatalog>,
    skill_name: &str,
) -> Result<Option<PathBuf>, ProviderError> {
    let Some(metadata) = catalog.get_skill(skill_name) else {
        return Ok(None);
    };
    if metadata.skill_path.is_empty() {
        return Ok(None);
    }
    let directory = PathBuf::from(metadata.skill_path);
    skill_content::check_publishable(&directory, skill_name)
        .map_err(|error| not_found(format!("skill `{skill_name}` is not published: {error}")))?;
    directory.canonicalize().map(Some).map_err(|error| {
        ProviderError::Internal(format!("cannot read skill `{skill_name}`: {error}"))
    })
}

fn directory_children(
    skill_name: &str,
    skill_root: &Path,
    directory: &Path,
) -> Result<Vec<McpResource>, ProviderError> {
    let read_dir = std::fs::read_dir(directory).map_err(|error| {
        ProviderError::Internal(format!("cannot read {}: {error}", directory.display()))
    })?;
    let mut resources = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|error| {
            ProviderError::Internal(format!("cannot read {}: {error}", directory.display()))
        })?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        let file_type = entry.file_type().map_err(|error| {
            ProviderError::Internal(format!("cannot read {}: {error}", path.display()))
        })?;
        let relative = path
            .strip_prefix(skill_root)
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| file_name.clone());
        let uri = skill_content::skill_uri(skill_name, &relative);
        if file_type.is_dir() {
            resources.push(McpResource {
                uri,
                name: file_name,
                description: None,
                mime_type: Some(DIRECTORY_MIME_TYPE.to_owned()),
            });
            continue;
        }
        if !file_type.is_file() {
            let Some(resolved) = skill_content::resolve_skill_file(skill_root, &relative) else {
                continue;
            };
            if !resolved.is_file() {
                continue;
            }
        }
        resources.push(McpResource {
            uri,
            name: file_name,
            description: None,
            mime_type: Some(skill_content::mime_type_for(&relative).to_owned()),
        });
    }
    Ok(resources)
}

fn not_found(message: String) -> ProviderError {
    ProviderError::NotFound(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_actions::ToolRegistry;
    use dcc_mcp_models::SkillMetadata;
    use std::sync::Arc;

    #[test]
    fn provider_publishes_manifest_and_reads_supporting_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("references")).expect("references");
        std::fs::write(
            temp.path().join("SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\n# Demo\n",
        )
        .expect("skill metadata");
        std::fs::write(temp.path().join("references/guide.md"), "# Guide\n")
            .expect("supporting file");

        let registry = Arc::new(ToolRegistry::new());
        let catalog = Arc::new(SkillCatalog::new(registry));
        catalog.add_skill(SkillMetadata {
            name: "demo".to_owned(),
            skill_path: temp.path().to_string_lossy().into_owned(),
            ..Default::default()
        });

        let provider = SkillCatalogProvider;
        let listed = provider.list_skills(&catalog);
        assert_eq!(listed.skills.len(), 1);
        let entry = &listed.skills[0];
        assert_eq!(entry.uri, "skill://demo/SKILL.md");
        assert_eq!(entry.frontmatter["license"], "MIT");

        let read = provider
            .read_skill_resource("skill://demo/references/guide.md", &catalog)
            .expect("read supporting file");
        assert_eq!(read.contents[0].text.as_deref(), Some("# Guide\n"));

        let digest = match &entry.resources {
            SkillResources::Entries(files) => files
                .iter()
                .find(|file| file.uri.ends_with("references/guide.md"))
                .expect("guide manifest entry")
                .digest
                .clone(),
            SkillResources::Dynamic => panic!("disk-backed skills are enumerated"),
        };
        assert!(digest.starts_with("sha256:"));
    }

    #[test]
    fn provider_rejects_skill_path_escape() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\n---\n",
        )
        .expect("skill metadata");
        let registry = Arc::new(ToolRegistry::new());
        let catalog = Arc::new(SkillCatalog::new(registry));
        catalog.add_skill(SkillMetadata {
            name: "demo".to_owned(),
            skill_path: temp.path().to_string_lossy().into_owned(),
            ..Default::default()
        });

        let error = SkillCatalogProvider
            .read_skill_resource("skill://demo/../SKILL.md", &catalog)
            .expect_err("path escape must fail");
        assert!(matches!(error, ProviderError::NotFound(_)));
    }
}
