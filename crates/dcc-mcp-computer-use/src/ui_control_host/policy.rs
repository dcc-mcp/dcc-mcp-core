use super::*;

pub(super) fn allows_owned_standard_menu_popup(action: &UiControlAction) -> bool {
    if action.input_kind != UiControlInputKind::RawInput || action.action != "keypress" {
        return false;
    }
    let mut navigation_keys = 0;
    for key in action.keys.iter().flat_map(|value| value.split('+')) {
        match key.trim().to_ascii_uppercase().as_str() {
            "ENTER" | "RETURN" | "ESC" | "ESCAPE" | "TAB" | "LEFT" | "ARROWLEFT" | "ARROW_LEFT"
            | "UP" | "ARROWUP" | "ARROW_UP" | "RIGHT" | "ARROWRIGHT" | "ARROW_RIGHT" | "DOWN"
            | "ARROWDOWN" | "ARROW_DOWN" | "HOME" | "END" | "PAGEUP" | "PAGE_UP" | "PGUP"
            | "PAGEDOWN" | "PAGE_DOWN" | "PGDN" => {
                navigation_keys += 1;
            }
            _ => return false,
        }
    }
    navigation_keys == 1
}

fn uses_stable_root_for_navigation(action: &UiControlAction) -> bool {
    allows_owned_standard_menu_popup(action)
        && !action
            .keys
            .iter()
            .flat_map(|value| value.split('+'))
            .any(|key| matches!(key.trim().to_ascii_uppercase().as_str(), "ENTER" | "RETURN"))
}

fn is_game_navigation(action: &UiControlAction) -> bool {
    action.input_kind == UiControlInputKind::RawInput && action.action == "game_navigation"
}

pub(super) fn classify_action(
    action: &UiControlAction,
    root: Option<&Value>,
    observation: Option<&Value>,
) -> UiControlPolicyTier {
    let mut tier = action.intent.policy_tier();
    if action.input_kind == UiControlInputKind::RawInput && action.action == "type" {
        return UiControlPolicyTier::HardDeny;
    }
    if action.input_kind == UiControlInputKind::RawInput
        && matches!(action.action.as_str(), "keypress" | "keyboard_shortcut")
        && crate::keyboard_policy::is_unmodified_text_entry(&action.keys)
    {
        return UiControlPolicyTier::HardDeny;
    }
    if is_game_navigation(action)
        && !root
            .and_then(find_focused_ancestry)
            .as_deref()
            .is_some_and(game_navigation_ancestry_is_safe)
    {
        return UiControlPolicyTier::HardDeny;
    }
    match crate::keyboard_policy::shortcut_risk(&action.keys) {
        crate::keyboard_policy::ShortcutRisk::HardDeny => {
            return UiControlPolicyTier::HardDeny;
        }
        crate::keyboard_policy::ShortcutRisk::ActionConfirmation => {
            tier = tier.max(UiControlPolicyTier::ActionConfirmation);
        }
        crate::keyboard_policy::ShortcutRisk::None => {}
    }

    let focused_input_action = matches!(
        action.action.as_str(),
        "type" | "keypress" | "keyboard_shortcut" | "game_navigation"
    );
    if focused_input_action && let Some(control) = root.and_then(find_focused_control) {
        tier = tier.max(classify_control(control));
    }
    if matches!(
        action.action.as_str(),
        "keypress" | "keyboard_shortcut" | "game_navigation"
    ) && let Some(ancestry) = root.and_then(find_focused_ancestry)
        && ancestry_has_authentication_secret_marker(&ancestry)
    {
        tier = UiControlPolicyTier::HardDeny;
    }

    if let Some(control_id) = action.control_id.as_deref()
        && let Some(control) = root.and_then(|root| find_control(root, control_id))
    {
        tier = tier.max(classify_control(control));
    }
    if action.input_kind == UiControlInputKind::Semantic
        && action.action == "set_text"
        && let Some(ancestry) = root.and_then(|root| {
            action
                .control_id
                .as_deref()
                .and_then(|control_id| find_control_ancestry(root, control_id))
        })
        && ancestry_has_authentication_secret_marker(&ancestry)
    {
        tier = UiControlPolicyTier::HardDeny;
    }
    if action.input_kind == UiControlInputKind::RawInput
        && let Some(root) = root
    {
        let mut classified_visual_target = false;
        for point in raw_action_points(action) {
            if let Some(control) = screenshot_to_desktop(point, observation)
                .and_then(|(x, y)| find_control_at_point(root, x, y))
            {
                classified_visual_target = true;
                tier = tier.max(classify_control(control));
            }
        }
        if !classified_visual_target && let Some(control) = find_focused_control(root) {
            tier = tier.max(classify_control(control));
        }
    }
    tier
}

fn raw_action_points(action: &UiControlAction) -> Vec<(f64, f64)> {
    let mut points = action.x.zip(action.y).into_iter().collect::<Vec<_>>();
    if action.action == "drag" {
        let path = action
            .path
            .iter()
            .map(|point| crate::ComputerUsePoint {
                x: point.x,
                y: point.y,
            })
            .collect::<Vec<_>>();
        points.extend(path.first().map(|point| (point.x, point.y)));
        points.extend(
            crate::drag_path::interpolated_drag_path(&path, action.duration_ms.unwrap_or(0))
                .into_iter()
                .map(|point| (point.x, point.y)),
        );
    } else {
        points.extend(action.path.iter().map(|point| (point.x, point.y)));
    }
    points
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActionControlFence {
    pub(super) identity: String,
    pub(super) is_password: bool,
    pub(super) name: String,
    pub(super) automation_id: String,
    pub(super) class_name: String,
    pub(super) policy_tier: UiControlPolicyTier,
}

fn action_control_fences(
    action: &UiControlAction,
    root: &Value,
    focus_runtime_id: Option<&str>,
    observation: Option<&Value>,
) -> Result<Vec<ActionControlFence>, HostFailure> {
    let controls = if action.input_kind == UiControlInputKind::Semantic {
        let control_id = action
            .control_id
            .as_deref()
            .ok_or_else(stale_accessibility_state)?;
        vec![find_control(root, control_id).ok_or_else(stale_accessibility_state)?]
    } else if is_game_navigation(action) {
        let focus_runtime_id = focus_runtime_id.ok_or_else(stale_accessibility_state)?;
        let ancestry =
            find_control_ancestry(root, focus_runtime_id).ok_or_else(stale_accessibility_state)?;
        if !game_navigation_ancestry_is_safe(&ancestry) {
            return Err(hard_denied_game_navigation_target());
        }
        // Game input may replace a short-lived focused canvas child while the
        // capability-bound top-level game window remains stable. Reclassify
        // the complete live focus ancestry above on every host and pre-input
        // pass, then fence the immutable root instead of a transient child.
        vec![root]
    } else if matches!(action.action.as_str(), "keypress" | "keyboard_shortcut")
        && (crate::keyboard_policy::is_modified_shortcut(&action.keys)
            || uses_stable_root_for_navigation(action))
    {
        // Application shortcuts are scoped by the immutable window capability,
        // the screen observation, and the native desktop/input fences. They do
        // not target a child UIA control. Non-activating navigation keys likewise
        // move or dismiss focus rather than invoke the focused control. Keep the
        // stable, capability-bound root fenced for those actions while the live
        // focus ancestry is classified on every host and pre-input pass; any root
        // or policy-tier change fails closed below. Enter/Return remains bound to
        // the exact focused control because it can invoke that control.
        vec![root]
    } else if matches!(action.action.as_str(), "keypress" | "keyboard_shortcut") {
        let focus_runtime_id = focus_runtime_id.ok_or_else(stale_accessibility_state)?;
        vec![find_control(root, focus_runtime_id).ok_or_else(stale_accessibility_state)?]
    } else {
        let points = raw_action_points(action);
        if points.is_empty() {
            let focus_runtime_id = focus_runtime_id.ok_or_else(stale_accessibility_state)?;
            return Ok(vec![control_fence(
                find_control(root, focus_runtime_id).ok_or_else(stale_accessibility_state)?,
            )?]);
        }
        points
            .into_iter()
            .map(|point| {
                let (x, y) = screenshot_to_desktop(point, observation)
                    .ok_or_else(stale_accessibility_state)?;
                find_control_at_point(root, x, y).ok_or_else(stale_accessibility_state)
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    controls.into_iter().map(control_fence).collect()
}

fn game_navigation_ancestry_is_safe(ancestry: &[&Value]) -> bool {
    let (Some(root), Some(focused)) = (ancestry.first(), ancestry.last()) else {
        return false;
    };
    let Some(root_process_id) = root.get("process_id").and_then(Value::as_u64) else {
        return false;
    };
    if root_process_id == 0
        || root.get("control_type").and_then(Value::as_str) != Some("ControlType.Window")
        || ancestry.iter().any(|control| {
            control.get("process_id").and_then(Value::as_u64) != Some(root_process_id)
                || control.get("is_password").and_then(Value::as_bool) != Some(false)
        })
        || !ancestry
            .iter()
            .skip(1)
            .all(|control| game_navigation_node_is_non_editable(control))
        || ancestry_has_authentication_secret_marker(ancestry)
    {
        return false;
    }
    matches!(
        focused.get("control_type").and_then(Value::as_str),
        Some("ControlType.Pane" | "ControlType.Custom" | "ControlType.Window")
    ) && game_navigation_node_is_non_editable(focused)
}

fn game_navigation_node_is_non_editable(control: &Value) -> bool {
    let Some(control_type) = control.get("control_type").and_then(Value::as_str) else {
        return false;
    };
    !matches!(control_type, "ControlType.Edit" | "ControlType.Document")
        && control.get("value").is_some_and(Value::is_null)
        && control
            .get("value_pattern_available")
            .and_then(Value::as_bool)
            == Some(false)
        && control
            .get("text_pattern_available")
            .and_then(Value::as_bool)
            == Some(false)
}

fn hard_denied_game_navigation_target() -> HostFailure {
    HostFailure::new(
        UiControlHostErrorCode::HardDenied,
        "game_navigation requires an exact non-editable game surface in the scoped DCC window",
    )
}

fn control_fence(control: &Value) -> Result<ActionControlFence, HostFailure> {
    let identity = ["runtime_id", "fallback_path"]
        .into_iter()
        .find_map(|key| {
            control
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        })
        .ok_or_else(stale_accessibility_state)?
        .to_owned();
    let text = |key| {
        control
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    Ok(ActionControlFence {
        identity,
        is_password: control
            .get("is_password")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        name: text("name"),
        automation_id: text("automation_id"),
        class_name: text("class_name"),
        policy_tier: classify_control(control),
    })
}

pub(super) fn stale_accessibility_state() -> HostFailure {
    HostFailure::new(
        UiControlHostErrorCode::StaleObservation,
        "the action-time UI Automation target differs from the latest host snapshot",
    )
}

pub(super) fn verify_action_fence(
    action: &UiControlAction,
    cached_root: &Value,
    cached_focus_runtime_id: Option<&str>,
    observation: Option<&Value>,
    live: &RuntimeAccessibilityState,
) -> Result<(UiControlPolicyTier, Vec<ActionControlFence>), HostFailure> {
    let cached = action_control_fences(action, cached_root, cached_focus_runtime_id, observation)?;
    let current = action_control_fences(
        action,
        &live.root,
        live.focus_runtime_id.as_deref(),
        observation,
    )?;
    if cached != current {
        return Err(stale_accessibility_state());
    }
    Ok((
        classify_action(action, Some(&live.root), observation),
        current,
    ))
}

#[cfg(any(windows, test))]
pub(super) fn verify_expected_action_fence(
    action: &UiControlAction,
    expected: &ActionFenceExpectation,
    live: &RuntimeAccessibilityState,
) -> Result<UiControlPolicyTier, HostFailure> {
    let current = action_control_fences(
        action,
        &live.root,
        live.focus_runtime_id.as_deref(),
        expected.observation.as_ref(),
    )?;
    if expected.controls != current {
        return Err(stale_accessibility_state());
    }
    let policy_tier = classify_action(action, Some(&live.root), expected.observation.as_ref());
    if policy_tier != expected.policy_tier {
        return Err(stale_accessibility_state());
    }
    Ok(policy_tier)
}

fn screenshot_to_desktop(point: (f64, f64), observation: Option<&Value>) -> Option<(f64, f64)> {
    let observation = observation?;
    let rect = observation.get("source_rect")?.as_array()?;
    if rect.len() != 4 {
        return None;
    }
    let source_x = rect[0].as_i64()? as f64;
    let source_y = rect[1].as_i64()? as f64;
    let source_width = rect[2].as_i64()? as f64;
    let source_height = rect[3].as_i64()? as f64;
    let image_width = observation.get("width")?.as_u64()? as f64;
    let image_height = observation.get("height")?.as_u64()? as f64;
    if source_width <= 0.0 || source_height <= 0.0 || image_width <= 0.0 || image_height <= 0.0 {
        return None;
    }
    Some((
        source_x + point.0 * source_width / image_width,
        source_y + point.1 * source_height / image_height,
    ))
}

pub(super) fn classify_control(control: &Value) -> UiControlPolicyTier {
    if control
        .get("is_password")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return UiControlPolicyTier::HardDeny;
    }
    let classification_fields = ["name", "automation_id", "class_name"]
        .iter()
        .filter_map(|key| control.get(key).and_then(Value::as_str))
        .collect::<Vec<_>>();
    let classification_text = classification_fields.join(" ").to_ascii_lowercase();
    let save_tier = if classification_fields
        .iter()
        .any(|value| is_common_save_label(value))
    {
        UiControlPolicyTier::ActionConfirmation
    } else {
        UiControlPolicyTier::TaskGrant
    };
    classify_control_text(&classification_text).max(save_tier)
}

pub(super) fn classify_control_text(text: &str) -> UiControlPolicyTier {
    const HARD_DENY: &[&str] = &[
        "password",
        "credential",
        "authentication code",
        "security settings",
        "privacy settings",
    ];
    const ALWAYS_CONFIRM: &[&str] = &[
        "delete",
        "remove permanently",
        "overwrite",
        "install",
        "purchase",
        "buy now",
        "pay",
        "send",
        "publish",
        "submit",
        "share",
        "grant access",
        "revoke access",
        "remote control",
        "remote connection",
        "allow remote",
    ];
    const PRE_APPROVE: &[&str] = &[
        "sign in",
        "log in",
        "login",
        "permission",
        "upload",
        "move",
        "rename",
        "connect account",
    ];
    if HARD_DENY.iter().any(|needle| text.contains(needle)) || is_authentication_secret_label(text)
    {
        UiControlPolicyTier::HardDeny
    } else if ALWAYS_CONFIRM.iter().any(|needle| text.contains(needle))
        || is_common_save_label(text)
    {
        UiControlPolicyTier::ActionConfirmation
    } else if PRE_APPROVE.iter().any(|needle| text.contains(needle)) {
        UiControlPolicyTier::PreApproval
    } else {
        UiControlPolicyTier::TaskGrant
    }
}

fn is_common_save_label(value: &str) -> bool {
    let normalized = normalized_control_label(value);
    matches!(
        normalized.as_str(),
        "save"
            | "save as"
            | "save button"
            | "save as button"
            | "save menu item"
            | "save as menu item"
            | "save command"
            | "save as command"
    )
}

fn is_authentication_secret_label(value: &str) -> bool {
    let normalized = normalized_control_label(value);
    normalized.contains("password")
        || normalized.contains("credential")
        || normalized.contains("authentication code")
        || normalized.contains("auth code")
        || normalized.contains("verification code")
        || normalized.contains("one time code")
        || normalized.contains("passcode")
        || normalized
            .split_whitespace()
            .any(|token| matches!(token, "otp" | "mfa" | "2fa"))
}

fn normalized_control_label(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn find_control<'a>(node: &'a Value, control_id: &str) -> Option<&'a Value> {
    if control_matches_id(node, control_id) {
        return Some(node);
    }
    node.get("children")
        .and_then(Value::as_array)
        .and_then(|children| {
            children
                .iter()
                .find_map(|child| find_control(child, control_id))
        })
}

fn find_control_ancestry<'a>(node: &'a Value, control_id: &str) -> Option<Vec<&'a Value>> {
    if control_matches_id(node, control_id) {
        return Some(vec![node]);
    }
    for child in node
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(mut ancestry) = find_control_ancestry(child, control_id) {
            ancestry.insert(0, node);
            return Some(ancestry);
        }
    }
    None
}

fn find_focused_ancestry(node: &Value) -> Option<Vec<&Value>> {
    if node.get("focused").and_then(Value::as_bool) == Some(true) {
        return Some(vec![node]);
    }
    for child in node
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(mut ancestry) = find_focused_ancestry(child) {
            ancestry.insert(0, node);
            return Some(ancestry);
        }
    }
    None
}

fn ancestry_has_authentication_secret_marker(ancestry: &[&Value]) -> bool {
    ancestry.iter().any(|control| {
        ["name", "automation_id", "class_name"]
            .iter()
            .filter_map(|key| control.get(key).and_then(Value::as_str))
            .any(is_authentication_secret_label)
    })
}

fn control_matches_id(node: &Value, control_id: &str) -> bool {
    if let Some(path) = control_id.strip_prefix("uia:path:") {
        node.get("fallback_path").and_then(Value::as_str) == Some(path)
    } else {
        let runtime_id = control_id.strip_prefix("uia:").unwrap_or(control_id);
        node.get("runtime_id").and_then(Value::as_str) == Some(runtime_id)
    }
}

fn find_control_at_point(node: &Value, x: f64, y: f64) -> Option<&Value> {
    let bounds = node.get("bounds")?;
    let left = bounds.get("x")?.as_f64()?;
    let top = bounds.get("y")?.as_f64()?;
    let width = bounds.get("width")?.as_f64()?;
    let height = bounds.get("height")?.as_f64()?;
    if x < left || y < top || x >= left + width || y >= top + height {
        return None;
    }
    node.get("children")
        .and_then(Value::as_array)
        .and_then(|children| {
            children
                .iter()
                .rev()
                .find_map(|child| find_control_at_point(child, x, y))
        })
        .or(Some(node))
}

fn find_focused_control(node: &Value) -> Option<&Value> {
    if node.get("focused").and_then(Value::as_bool) == Some(true) {
        return Some(node);
    }
    node.get("children")
        .and_then(Value::as_array)
        .and_then(|children| children.iter().find_map(find_focused_control))
}
