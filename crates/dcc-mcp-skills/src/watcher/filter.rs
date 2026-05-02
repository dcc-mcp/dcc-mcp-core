//! Filesystem-event filtering for [`SkillWatcher`](super::SkillWatcher).

use std::path::Path;

use notify::{Event, EventKind};

/// Determine whether a notify event should trigger a skill reload.
///
/// We reload on Create/Modify/Remove events for any file whose name
/// matches skill-related patterns (SKILL.md, .py, .mel, .lua, etc.)
/// or any directory event (a new skill subdirectory may have appeared).
pub(crate) fn should_reload(event: &Event) -> bool {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
            // Reload if the changed path looks like a skill file or directory
            event.paths.iter().any(|p| is_skill_related(p))
        }
        _ => false,
    }
}

/// Return `true` if `path` is likely to affect skill loading.
pub(crate) fn is_skill_related(path: &Path) -> bool {
    // Always reload for directory events — a new skill directory may appear
    if path.is_dir() {
        return true;
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    // SKILL.md itself
    if file_name.eq_ignore_ascii_case("skill.md") {
        return true;
    }

    // depends.md inside metadata/
    if file_name.eq_ignore_ascii_case("depends.md") {
        return true;
    }

    // Script files (check extension against supported list)
    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && crate::constants::is_supported_extension(ext)
    {
        return true;
    }

    false
}
