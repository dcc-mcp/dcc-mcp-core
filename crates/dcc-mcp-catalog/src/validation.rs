use super::{CatalogEntry, CatalogInstall};
use crate::CatalogValidationError;

// ── schema validation ─────────────────────────────────────────────────────────

/// JSON Schema (Draft 2020-12) for marketplace-v2 catalog entries.
///
/// Each entry must declare at least `name` and `description`; all other
/// fields are optional.  `additionalProperties: false` on both the top-level
/// document and each entry catches typos early.
const MARKETPLACE_V2_SCHEMA_JSON: &str = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://dcc-mcp.github.io/schemas/marketplace-v2.schema.json",
  "title": "DCC-MCP Marketplace Catalog",
  "description": "Schema for marketplace.json catalog entries",
  "type": "object",
  "required": ["entries"],
  "properties": {
    "version": { "type": "string" },
    "entries": {
      "type": "array",
      "items": { "$ref": "#/$defs/entry" }
    }
  },
  "additionalProperties": false,
  "$defs": {
    "entry": {
      "type": "object",
      "required": ["name", "description"],
      "properties": {
        "name":        { "type": "string", "minLength": 1 },
        "description": { "type": "string", "minLength": 1 },
        "dcc":         { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
        "targets": {
          "type": "array",
          "minItems": 1,
          "items": {
            "type": "object",
            "required": ["kind", "id"],
            "properties": {
              "kind": { "type": "string", "enum": ["dcc", "application", "game", "web"] },
              "id": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
          },
          "uniqueItems": true
        },
        "url":         { "type": "string" },
        "tags":        { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
        "version":          { "type": "string" },
        "min_core_version": { "type": "string" },
        "maintainer":       { "type": "string" },
        "category":         { "type": "string" },
        "policy": {
          "type": "object",
          "required": ["installation"],
          "properties": {
            "installation": { "type": "string" }
          },
          "additionalProperties": false
        },
        "requires": {
          "type": "object",
          "properties": {
            "env": { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
            "bins": { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
            "python": { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
            "skills": { "type": "array", "items": { "type": "string" }, "uniqueItems": true }
          },
          "additionalProperties": false
        },
        "icon":        { "type": "string" },
        "showcase": {
          "type": "string",
          "pattern": "^(https?://[^\\s]+|[A-Za-z0-9][A-Za-z0-9._-]*(/[A-Za-z0-9][A-Za-z0-9._-]*)*\\.(png|jpg|jpeg|webp|avif|gif))$"
        },
        "package": {
          "type": "object",
          "required": ["format"],
          "properties": {
            "format": { "type": "string", "enum": ["skill", "agent-plugin", "skill-bundle", "cua-profile", "composite"] },
            "skills": {
              "type": "array",
              "items": { "type": "string", "minLength": 1 },
              "uniqueItems": true
            },
            "components": {
              "type": "array",
              "items": {
                "type": "object",
                "required": ["kind", "id", "root"],
                "properties": {
                  "kind": { "type": "string", "enum": ["skill", "cua-profile"] },
                  "id": { "type": "string", "minLength": 1 },
                  "root": { "type": "string", "minLength": 1 }
                },
                "additionalProperties": false
              }
            }
          },
          "allOf": [
            {
              "if": { "properties": { "format": { "enum": ["skill", "agent-plugin", "skill-bundle"] } } },
              "then": { "required": ["skills"], "properties": { "skills": { "minItems": 1 } } }
            },
            {
              "if": { "properties": { "format": { "const": "skill-bundle" } } },
              "then": { "properties": { "skills": { "minItems": 2 } } }
            },
            {
              "if": { "properties": { "format": { "const": "cua-profile" } } },
              "then": { "required": ["components"], "properties": { "components": { "minItems": 1, "maxItems": 1 } } }
            }
          ],
          "additionalProperties": false
        },
        "install": {
          "type": "object",
          "required": ["type"],
          "properties": {
            "type":        { "type": "string", "enum": ["git", "zip", "path", "pip"] },
            "url":         { "type": "string" },
            "ref":         { "type": "string" },
            "sha256":      { "type": "string" },
            "skillRoots":  { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
            "pip_package": { "type": "string", "pattern": "^[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$" },
            "pip_extras":  { "type": "array", "items": { "type": "string", "pattern": "^[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$" }, "uniqueItems": true },
            "python_path": { "type": "string" },
            "entry_point": { "type": "string" },
            "instructions_url": { "type": "string" }
          },
          "allOf": [
            {
              "if": { "properties": { "type": { "const": "git" } } },
              "then": {
                "required": ["url", "ref"],
                "properties": { "ref": { "pattern": "^[0-9a-fA-F]{40}$" } }
              }
            },
            {
              "if": { "properties": { "type": { "const": "zip" } } },
              "then": {
                "required": ["url", "sha256"],
                "properties": { "sha256": { "pattern": "^(sha256:)?[0-9a-fA-F]{64}$" } }
              }
            },
            {
              "if": { "properties": { "type": { "const": "pip" } } },
              "then": {
                "required": ["url", "sha256", "pip_package"],
                "properties": {
                  "url": { "pattern": "^https://" },
                  "sha256": { "pattern": "^(sha256:)?[0-9a-fA-F]{64}$" },
                  "pip_package": { "minLength": 1 }
                }
              }
            }
          ],
          "additionalProperties": false
        }
      },
      "additionalProperties": false
    }
  }
}"##;

/// Validate a single [`CatalogEntry`] against the marketplace-v2 JSON Schema.
///
/// Returns `Ok(())` if the entry is valid, or a
/// [`CatalogValidationError::ValidationFailed`] with a human-readable message
/// describing what failed.
pub fn validate_entry(entry: &CatalogEntry) -> Result<(), CatalogValidationError> {
    let value = serde_json::to_value(entry).map_err(|e| {
        CatalogValidationError::SchemaError(format!(
            "failed to serialize entry '{}' for validation: {e}",
            entry.name
        ))
    })?;

    let schema = entry_schema()?;
    let validation = schema.validate(&value);
    if let Err(err) = validation {
        return Err(CatalogValidationError::ValidationFailed {
            name: entry.name.clone(),
            message: format!("  - {}: {}", err.instance_path, err),
        });
    }
    if let Some(install) = entry
        .install
        .as_ref()
        .filter(|install| install.install_type == "pip")
    {
        validate_pip_artifact_binding(entry, install)?;
    }
    Ok(())
}

fn validate_pip_artifact_binding(
    entry: &CatalogEntry,
    install: &CatalogInstall,
) -> Result<(), CatalogValidationError> {
    let package = install.pip_package.as_deref().unwrap_or_default().trim();
    let version = entry.version.as_deref().unwrap_or_default().trim();
    let artifact_url = install.url.as_deref().unwrap_or_default().trim();
    let filename = artifact_url.rsplit('/').next().unwrap_or_default();
    let normalized_package = package
        .chars()
        .map(|character| match character {
            '-' | '.' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect::<String>();
    let expected_prefix = format!("{normalized_package}-{version}-");
    let valid = !version.is_empty()
        && !artifact_url.contains('#')
        && !artifact_url.contains('?')
        && filename
            .to_ascii_lowercase()
            .starts_with(&expected_prefix.to_ascii_lowercase())
        && filename.ends_with("-py3-none-any.whl");
    if valid {
        return Ok(());
    }
    Err(CatalogValidationError::ValidationFailed {
        name: entry.name.clone(),
        message: format!(
            "  - /install/url: pip artifact must be an immutable py3-none-any wheel for {package}=={version}"
        ),
    })
}

/// Validate a slice of [`CatalogEntry`] against the marketplace-v2 JSON Schema.
///
/// Returns `Ok(())` if all entries pass, or
/// [`CatalogValidationError::MultipleFailures`] aggregating each failed entry.
pub fn validate_catalog_entries(entries: &[CatalogEntry]) -> Result<(), CatalogValidationError> {
    let mut failures = Vec::new();
    for entry in entries {
        if let Err(err) = validate_entry(entry) {
            failures.push(err);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        let count = failures.len();
        Err(CatalogValidationError::MultipleFailures { count, failures })
    }
}

/// Compile the entry sub-schema once from `$defs/entry`.
fn entry_schema() -> Result<jsonschema::Validator, CatalogValidationError> {
    let schema_value: serde_json::Value = serde_json::from_str(MARKETPLACE_V2_SCHEMA_JSON)
        .map_err(|e| {
            CatalogValidationError::SchemaError(format!("invalid embedded schema: {e}"))
        })?;
    let entry_schema_value = schema_value
        .pointer("/$defs/entry")
        .cloned()
        .ok_or_else(|| {
            CatalogValidationError::SchemaError("missing $defs/entry in schema".into())
        })?;
    jsonschema::validator_for(&entry_schema_value)
        .map_err(|e| CatalogValidationError::SchemaError(format!("failed to compile schema: {e}")))
}
