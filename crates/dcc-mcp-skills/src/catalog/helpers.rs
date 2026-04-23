use super::*;

/// Parse a scope filter string (case-insensitive) into a [`SkillScope`].
///
/// Accepts: `"repo"`, `"user"`, `"system"`, `"admin"`. Used by the unified
/// `search_skills` entry point (#340) so Python and JSON-RPC callers can
/// filter by trust level without crossing the PyO3 enum boundary directly.
#[cfg_attr(not(any(feature = "python-bindings", test)), allow(dead_code))]
pub(crate) fn parse_scope_str(s: &str) -> Result<SkillScope, String> {
    match s.to_ascii_lowercase().as_str() {
        "repo" => Ok(SkillScope::Repo),
        "user" => Ok(SkillScope::User),
        "system" => Ok(SkillScope::System),
        "admin" => Ok(SkillScope::Admin),
        other => Err(format!(
            "invalid scope {other:?}: expected 'repo' | 'user' | 'system' | 'admin'"
        )),
    }
}

/// Convert a SkillEntry into a SkillSummary.
///
/// The `search_hint` falls back to `description` if not set in SKILL.md.
pub fn skill_entry_to_summary(e: &SkillEntry) -> SkillSummary {
    SkillSummary {
        name: e.metadata.name.clone(),
        description: e.metadata.description.clone(),
        search_hint: if e.metadata.search_hint.is_empty() {
            e.metadata.description.clone()
        } else {
            e.metadata.search_hint.clone()
        },
        tags: e.metadata.tags.clone(),
        dcc: e.metadata.dcc.clone(),
        version: e.metadata.version.clone(),
        tool_count: e.metadata.tools.len(),
        tool_names: e.metadata.tools.iter().map(|t| t.name.clone()).collect(),
        loaded: e.state == SkillState::Loaded,
        scope: e.scope.label().to_string(),
        implicit_invocation: e
            .metadata
            .policy
            .as_ref()
            .map(|p| p.is_implicit_invocation_allowed())
            .unwrap_or(true),
    }
}

/// Drop tool names in `next-tools` that fail `validate_tool_name` so
/// the catalog never surfaces malformed follow-up suggestions to AI
/// clients (issue #342).
///
/// Invalid entries are logged at warn-level and skipped; skill load
/// succeeds so a typo in one tool's `next-tools` list does not block
/// an entire skill.
pub fn sanitize_next_tools(
    raw: &dcc_mcp_models::NextTools,
    skill_name: &str,
    action_name: &str,
) -> dcc_mcp_models::NextTools {
    let sanitize = |kind: &str, names: &[String]| -> Vec<String> {
        names
            .iter()
            .filter_map(|n| match dcc_mcp_naming::validate_tool_name(n) {
                Ok(()) => Some(n.clone()),
                Err(e) => {
                    tracing::warn!(
                        "skill {skill_name}: tool {action_name}: next-tools.{kind} entry \
                         {n:?} is not a valid tool name ({e}); dropping.",
                    );
                    None
                }
            })
            .collect()
    };
    dcc_mcp_models::NextTools {
        on_success: sanitize("on-success", &raw.on_success),
        on_failure: sanitize("on-failure", &raw.on_failure),
    }
}
