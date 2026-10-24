use std::path::Path;

use dcc_mcp_skills::gui_executable::{GuiExecutableHint, is_gui_executable};

/// Select a pip interpreter without executing any host or probing PATH.
pub(crate) fn resolve_python(
    override_python: Option<String>,
    catalog_python: Option<String>,
    dcc_path: Option<&Path>,
    dcc_type: &str,
) -> Result<Option<String>, GuiExecutableHint> {
    if let Some(python) = override_python.or(catalog_python) {
        if let Some(hint) = is_gui_executable(Path::new(&python)) {
            return Err(hint);
        }
        return Ok(Some(python));
    }
    let Some(path) = dcc_path else {
        return Ok(None);
    };
    let Some(hint) = is_gui_executable(path) else {
        return Ok(None);
    };
    // Commandlet/GUI replacements are not generic `python -m pip` interpreters.
    if hint.dcc_kind != dcc_type || !matches!(hint.dcc_kind, "maya" | "houdini") {
        return Ok(None);
    }
    if !path.is_file() {
        return Err(hint);
    }
    let python = hint
        .recommended_replacement
        .as_ref()
        .filter(|path| path.is_file())
        .and_then(|path| path.to_str())
        .map(str::to_owned);
    python.map(Some).ok_or(hint)
}
