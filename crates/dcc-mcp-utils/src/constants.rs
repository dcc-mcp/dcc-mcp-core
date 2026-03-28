//! Constants for the DCC-MCP ecosystem.

use std::collections::HashMap;
use std::sync::LazyLock;

pub const APP_NAME: &str = "dcc-mcp";
pub const APP_AUTHOR: &str = "dcc-mcp";
pub const LOG_APP_NAME: &str = "dcc-mcp-core";
pub const DEFAULT_LOG_LEVEL: &str = "DEBUG";
pub const ENV_LOG_LEVEL: &str = "MCP_LOG_LEVEL";
pub const ENV_ACTION_PATH_PREFIX: &str = "DCC_MCP_ACTION_PATH_";
pub const ENV_ACTIONS_DIR: &str = "DCC_MCP_ACTIONS_DIR";
pub const SKILL_METADATA_FILE: &str = "SKILL.md";
pub const ENV_SKILL_PATHS: &str = "DCC_MCP_SKILL_PATHS";
pub const SKILL_SCRIPTS_DIR: &str = "scripts";
pub const DEFAULT_DCC: &str = "python";

/// Supported script extensions → script type name.
pub static SUPPORTED_SCRIPT_EXTENSIONS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert(".py", "python");
    m.insert(".mel", "mel");
    m.insert(".ms", "maxscript");
    m.insert(".bat", "batch");
    m.insert(".cmd", "batch");
    m.insert(".sh", "shell");
    m.insert(".bash", "shell");
    m.insert(".ps1", "powershell");
    m.insert(".vbs", "vbscript");
    m.insert(".jsx", "javascript");
    m.insert(".js", "javascript");
    m
});

/// Boolean flag keys for parameter processing.
pub const BOOLEAN_FLAG_KEYS: &[&str] = &[
    "query", "q", "edit", "e", "select", "sl", "selection", "visible", "v", "hidden", "h",
];
