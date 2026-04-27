//! Skill-domain constants — moved from `dcc-mcp-utils` (issue #498).
//!
//! Skill-specific filenames, environment variable names, supported script
//! extensions and the file-mtime epsilon used by [`SkillScanner`] all live
//! here so that crates that only need filesystem helpers do not transitively
//! pull in skill-domain knowledge.

/// Filename expected at the root of every skill package.
pub const SKILL_METADATA_FILE: &str = "SKILL.md";

/// Environment variable containing additional skill search paths.
pub const ENV_SKILL_PATHS: &str = "DCC_MCP_SKILL_PATHS";

/// Template for per-app skill search paths: `DCC_MCP_{APP}_SKILL_PATHS`.
/// Substitute `{APP}` with the uppercase DCC app name.
/// Example: `DCC_MCP_MAYA_SKILL_PATHS`, `DCC_MCP_BLENDER_SKILL_PATHS`.
pub const ENV_APP_SKILL_PATHS_TEMPLATE: &str = "DCC_MCP_{APP}_SKILL_PATHS";

/// Environment variable containing user-level accumulated skill search paths.
pub const ENV_USER_SKILL_PATHS: &str = "DCC_MCP_USER_SKILL_PATHS";

/// Template for per-app user skill search paths: `DCC_MCP_USER_{APP}_SKILL_PATHS`.
pub const ENV_USER_APP_SKILL_PATHS_TEMPLATE: &str = "DCC_MCP_USER_{APP}_SKILL_PATHS";

/// Environment variable containing team-level accumulated skill search paths.
pub const ENV_TEAM_SKILL_PATHS: &str = "DCC_MCP_TEAM_SKILL_PATHS";

/// Template for per-app team skill search paths: `DCC_MCP_TEAM_{APP}_SKILL_PATHS`.
pub const ENV_TEAM_APP_SKILL_PATHS_TEMPLATE: &str = "DCC_MCP_TEAM_{APP}_SKILL_PATHS";

/// Environment variable to disable automatic discovery of accumulated skills.
pub const ENV_DISABLE_ACCUMULATED_SKILLS: &str = "DCC_MCP_DISABLE_ACCUMULATED_SKILLS";

/// Subdirectory inside a skill package that holds executable scripts.
pub const SKILL_SCRIPTS_DIR: &str = "scripts";

/// Subdirectory inside a skill package that holds auxiliary metadata files.
pub const SKILL_METADATA_DIR: &str = "metadata";

/// Filename for the dependency listing inside the metadata/ directory.
pub const DEPENDS_FILE: &str = "depends.md";

/// Tolerance in seconds for file modification time comparison in cache checks.
pub const MTIME_EPSILON_SECS: f64 = 0.001;

/// Build the per-app env var name for a given app/DCC name.
///
/// `app_name = "maya"` → `"DCC_MCP_MAYA_SKILL_PATHS"`
#[must_use]
pub fn app_skill_paths_env_key(app_name: &str) -> String {
    format!(
        "DCC_MCP_{}_SKILL_PATHS",
        app_name.to_uppercase().replace('-', "_")
    )
}

/// Build the per-app user skill paths env var name.
///
/// `app_name = "maya"` → `"DCC_MCP_USER_MAYA_SKILL_PATHS"`
#[must_use]
pub fn user_skill_paths_env_key(app_name: &str) -> String {
    format!(
        "DCC_MCP_USER_{}_SKILL_PATHS",
        app_name.to_uppercase().replace('-', "_")
    )
}

/// Build the per-app team skill paths env var name.
///
/// `app_name = "maya"` → `"DCC_MCP_TEAM_MAYA_SKILL_PATHS"`
#[must_use]
pub fn team_skill_paths_env_key(app_name: &str) -> String {
    format!(
        "DCC_MCP_TEAM_{}_SKILL_PATHS",
        app_name.to_uppercase().replace('-', "_")
    )
}

/// Supported script extensions → script type name (compile-time constant).
pub const SUPPORTED_SCRIPT_EXTENSIONS: &[(&str, &str)] = &[
    (".py", "python"),
    (".mel", "mel"),
    (".ms", "maxscript"),
    (".bat", "batch"),
    (".cmd", "batch"),
    (".sh", "shell"),
    (".bash", "shell"),
    (".ps1", "powershell"),
    (".vbs", "vbscript"),
    (".jsx", "javascript"),
    (".js", "javascript"),
];

/// Normalize an extension to bare form (strip optional leading dot).
fn normalize_ext(ext: &str) -> &str {
    ext.strip_prefix('.').unwrap_or(ext)
}

/// Check if a file extension is a supported script extension.
///
/// Accepts both dotted (`.py`) and bare (`py`) forms; comparison is case-insensitive.
#[must_use]
pub fn is_supported_extension(ext: &str) -> bool {
    let bare = normalize_ext(ext);
    SUPPORTED_SCRIPT_EXTENSIONS.iter().any(|(e, _)| {
        e.strip_prefix('.')
            .is_some_and(|b| b.eq_ignore_ascii_case(bare))
    })
}

/// Look up the script type name for a file extension.
///
/// Accepts both dotted (`.py`) and bare (`py`) forms; comparison is case-insensitive.
/// Returns `None` if the extension is not recognized.
#[must_use]
pub fn get_script_type(ext: &str) -> Option<&'static str> {
    let bare = normalize_ext(ext);
    SUPPORTED_SCRIPT_EXTENSIONS.iter().find_map(|(e, t)| {
        e.strip_prefix('.')
            .filter(|b| b.eq_ignore_ascii_case(bare))
            .map(|_| *t)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported_extension_valid() {
        assert!(is_supported_extension(".py"));
        assert!(is_supported_extension("py"));
        assert!(is_supported_extension(".sh"));
        assert!(is_supported_extension("sh"));
        assert!(is_supported_extension(".js"));
        assert!(is_supported_extension("PY")); // case-insensitive
    }

    #[test]
    fn test_is_supported_extension_all_supported() {
        for (ext, _) in SUPPORTED_SCRIPT_EXTENSIONS {
            assert!(
                is_supported_extension(ext),
                "expected {ext} to be supported"
            );
            let bare = ext.strip_prefix('.').unwrap_or(ext);
            assert!(is_supported_extension(bare), "bare form {bare} should work");
        }
    }

    #[test]
    fn test_is_supported_extension_invalid() {
        assert!(!is_supported_extension(".txt"));
        assert!(!is_supported_extension(".rs"));
        assert!(!is_supported_extension("txt"));
        assert!(!is_supported_extension(""));
        assert!(!is_supported_extension("toml"));
        assert!(!is_supported_extension(".json"));
    }

    #[test]
    fn test_is_supported_extension_case_insensitive() {
        assert!(is_supported_extension("PY"));
        assert!(is_supported_extension(".MEL"));
        assert!(is_supported_extension("MS"));
        assert!(is_supported_extension(".PS1"));
    }

    #[test]
    fn test_get_script_type() {
        assert_eq!(get_script_type(".py"), Some("python"));
        assert_eq!(get_script_type("py"), Some("python"));
        assert_eq!(get_script_type(".sh"), Some("shell"));
        assert_eq!(get_script_type(".bat"), Some("batch"));
        assert_eq!(get_script_type(".js"), Some("javascript"));
        assert_eq!(get_script_type(".mel"), Some("mel"));
        assert_eq!(get_script_type(".txt"), None);
        assert_eq!(get_script_type("PY"), Some("python"));
    }

    #[test]
    fn test_get_script_type_all_supported() {
        for (ext, expected_type) in SUPPORTED_SCRIPT_EXTENSIONS {
            assert_eq!(
                get_script_type(ext),
                Some(*expected_type),
                "ext={ext} should map to {expected_type}"
            );
        }
    }

    #[test]
    fn test_get_script_type_unknown_returns_none() {
        assert!(get_script_type(".rs").is_none());
        assert!(get_script_type(".md").is_none());
        assert!(get_script_type("").is_none());
    }

    #[test]
    fn test_get_script_type_variants() {
        assert_eq!(get_script_type(".bat"), Some("batch"));
        assert_eq!(get_script_type(".cmd"), Some("batch"));
        assert_eq!(get_script_type(".sh"), Some("shell"));
        assert_eq!(get_script_type(".bash"), Some("shell"));
        assert_eq!(get_script_type(".jsx"), Some("javascript"));
        assert_eq!(get_script_type(".js"), Some("javascript"));
    }

    #[test]
    fn test_mtime_epsilon_positive() {
        assert_eq!(MTIME_EPSILON_SECS, 0.001);
    }

    #[test]
    fn test_skill_metadata_file_constant() {
        assert_eq!(SKILL_METADATA_FILE, "SKILL.md");
        assert!(SKILL_METADATA_FILE.ends_with(".md"));
    }

    #[test]
    fn test_env_var_constants_not_empty() {
        assert_eq!(ENV_SKILL_PATHS, "DCC_MCP_SKILL_PATHS");
        assert_eq!(ENV_USER_SKILL_PATHS, "DCC_MCP_USER_SKILL_PATHS");
        assert_eq!(ENV_TEAM_SKILL_PATHS, "DCC_MCP_TEAM_SKILL_PATHS");
    }

    #[test]
    fn test_env_key_helpers() {
        assert_eq!(app_skill_paths_env_key("maya"), "DCC_MCP_MAYA_SKILL_PATHS");
        assert_eq!(
            user_skill_paths_env_key("maya"),
            "DCC_MCP_USER_MAYA_SKILL_PATHS"
        );
        assert_eq!(
            team_skill_paths_env_key("maya"),
            "DCC_MCP_TEAM_MAYA_SKILL_PATHS"
        );
    }
}
