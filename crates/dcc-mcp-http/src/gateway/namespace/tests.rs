//! Unit tests for the namespace module.

use super::*;
use dcc_mcp_naming::validate_tool_name;
use uuid::Uuid;

#[test]
fn instance_short_deterministic() {
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    assert_eq!(instance_short(&id), "abcdef01");
}

#[test]
fn encode_uses_dot_separator() {
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    let enc = encode_tool_name(&id, "create_sphere");
    assert_eq!(enc, "abcdef01.create_sphere");
}

#[test]
fn encode_never_contains_slash() {
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    for tool in [
        "create_sphere",
        "maya-animation.set_keyframe",
        "CamelCase",
        "x",
    ] {
        let enc = encode_tool_name(&id, tool);
        assert!(
            !enc.contains('/'),
            "gateway encoded {tool:?} -> {enc:?} which contains `/`"
        );
    }
}

#[test]
fn encode_produces_sep986_compliant_names() {
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    for tool in ["create_sphere", "maya-animation.set_keyframe", "CamelCase"] {
        let enc = encode_tool_name(&id, tool);
        assert!(
            validate_tool_name(&enc).is_ok(),
            "gateway emitted {enc:?} which fails SEP-986 validation"
        );
    }
}

#[test]
fn encode_then_decode_roundtrips() {
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    let enc = encode_tool_name(&id, "maya-animation.set_keyframe");
    assert_eq!(enc, "abcdef01.maya-animation.set_keyframe");
    let (p, o) = decode_tool_name(&enc).unwrap();
    assert_eq!(p, "abcdef01");
    assert_eq!(o, "maya-animation.set_keyframe");
}

#[test]
fn decode_accepts_preferred_dot_form() {
    let (p, n) = decode_tool_name("abcdef01.create_sphere").unwrap();
    assert_eq!(p, "abcdef01");
    assert_eq!(n, "create_sphere");
}

#[test]
fn decode_accepts_deprecated_slash_form() {
    let (p, n) = decode_tool_name("abcdef01/create_sphere").unwrap();
    assert_eq!(p, "abcdef01");
    assert_eq!(n, "create_sphere");
}

#[test]
fn decode_accepts_legacy_double_underscore_form() {
    let (p, n) = decode_tool_name("abcdef01__maya_geometry__create_sphere").unwrap();
    assert_eq!(p, "abcdef01");
    assert_eq!(n, "maya_geometry__create_sphere");
}

#[test]
fn decode_rejects_non_hex_prefix() {
    // 8 chars but not hex → must not be mistaken for an instance prefix.
    assert!(decode_tool_name("zzzzzzzz.create").is_none());
}

#[test]
fn decode_rejects_wrong_length_prefix() {
    assert!(decode_tool_name("abcdef.create").is_none()); // 6 chars
    assert!(decode_tool_name("abcdef012.create").is_none()); // 9 chars
}

#[test]
fn local_tools_decode_to_none() {
    for name in GATEWAY_LOCAL_TOOLS {
        assert!(decode_tool_name(name).is_none());
    }
}

#[test]
fn extract_bare_name_strips_prefix() {
    assert_eq!(
        extract_bare_tool_name("maya-animation", "maya_animation__set_keyframe"),
        "set_keyframe"
    );
    assert_eq!(
        extract_bare_tool_name("", "get_scene_info"),
        "get_scene_info"
    );
}

#[test]
fn skill_tool_name_formats_correctly() {
    assert_eq!(
        skill_tool_name("maya-animation", "maya_animation__set_keyframe"),
        Some("maya-animation.set_keyframe".to_string())
    );
    assert_eq!(skill_tool_name("", "set_keyframe"), None);
}

#[test]
fn skill_tool_name_none_for_core_tools() {
    for core in CORE_TOOL_NAMES {
        assert_eq!(skill_tool_name("s", &format!("s__{core}")), None);
    }
}

#[test]
fn decode_skill_tool_name_round_trips() {
    let (skill, tool) = decode_skill_tool_name("maya-animation.set_keyframe").unwrap();
    assert_eq!(skill, "maya-animation");
    assert_eq!(tool, "set_keyframe");
}

#[test]
fn decode_skill_tool_name_rejects_stubs() {
    assert!(decode_skill_tool_name("__skill__maya").is_none());
    assert!(decode_skill_tool_name("abcdef01/tool.name").is_none());
}

#[test]
fn decode_skill_tool_name_rejects_gateway_encoded_form() {
    // `abcdef01.create_sphere` is a gateway-encoded tool, not a skill.tool
    // pair. `decode_skill_tool_name` must yield None so callers route it
    // through `decode_tool_name` instead.
    assert!(decode_skill_tool_name("abcdef01.create_sphere").is_none());
}

#[test]
fn assert_gateway_tool_name_accepts_compliant() {
    assert!(assert_gateway_tool_name("abcdef01.create_sphere").is_ok());
}

#[test]
fn assert_gateway_tool_name_rejects_slash() {
    assert!(assert_gateway_tool_name("abcdef01/create_sphere").is_err());
}

// ── #307 bare-name resolver ──────────────────────────────────────────────

#[test]
fn bare_name_when_unique_within_instance() {
    __reset_warn_state_for_tests();
    let inputs = [
        BareNameInput {
            skill_name: "maya-anim",
            action_name: "maya_anim__set_keyframe",
        },
        BareNameInput {
            skill_name: "maya-geo",
            action_name: "maya_geo__create_sphere",
        },
    ];
    let bare = resolve_bare_names(&inputs);
    assert!(bare.contains(&(
        "maya-anim".to_string(),
        "maya_anim__set_keyframe".to_string()
    )));
    assert!(bare.contains(&(
        "maya-geo".to_string(),
        "maya_geo__create_sphere".to_string()
    )));
    assert_eq!(bare.len(), 2);
}

#[test]
fn falls_back_to_skill_prefix_on_collision() {
    __reset_warn_state_for_tests();
    // Both skills expose a `set_keyframe` action → collision; neither
    // should be emitted bare.
    let inputs = [
        BareNameInput {
            skill_name: "maya-anim",
            action_name: "maya_anim__set_keyframe",
        },
        BareNameInput {
            skill_name: "blender-anim",
            action_name: "blender_anim__set_keyframe",
        },
        BareNameInput {
            skill_name: "maya-geo",
            action_name: "maya_geo__create_sphere",
        },
    ];
    let bare = resolve_bare_names(&inputs);
    assert!(bare.contains(&(
        "maya-geo".to_string(),
        "maya_geo__create_sphere".to_string()
    )));
    assert!(!bare.contains(&(
        "maya-anim".to_string(),
        "maya_anim__set_keyframe".to_string()
    )));
    assert!(!bare.contains(&(
        "blender-anim".to_string(),
        "blender_anim__set_keyframe".to_string()
    )));
}

#[test]
fn same_skill_registering_same_bare_twice_is_not_a_collision() {
    __reset_warn_state_for_tests();
    // Re-registering the same (skill, action) shape twice must not be
    // mistaken for a cross-skill collision.
    let inputs = [
        BareNameInput {
            skill_name: "maya-anim",
            action_name: "maya_anim__set_keyframe",
        },
        BareNameInput {
            skill_name: "maya-anim",
            action_name: "maya_anim__set_keyframe",
        },
    ];
    let bare = resolve_bare_names(&inputs);
    assert!(bare.contains(&(
        "maya-anim".to_string(),
        "maya_anim__set_keyframe".to_string()
    )));
}

#[test]
fn core_tool_names_are_never_bare_eligible() {
    __reset_warn_state_for_tests();
    // `load_skill` is reserved and already has a first-class position
    // in tools/list; emitting it bare from a skill would cause a dispatch
    // ambiguity against the meta-tool.
    let inputs = [BareNameInput {
        skill_name: "rogue-skill",
        action_name: "rogue_skill__load_skill",
    }];
    assert!(resolve_bare_names(&inputs).is_empty());
}

#[test]
fn actions_without_skill_are_skipped_by_resolver() {
    __reset_warn_state_for_tests();
    // Actions not registered from a skill keep their canonical name;
    // the resolver simply ignores them rather than asserting they are
    // unique.
    let inputs = [BareNameInput {
        skill_name: "",
        action_name: "standalone_action",
    }];
    assert!(resolve_bare_names(&inputs).is_empty());
}

#[test]
fn warn_legacy_prefixed_once_is_one_shot_per_name() {
    __reset_warn_state_for_tests();
    // Two calls with the same name should not panic or repeatedly warn;
    // we can only verify the API surface is idempotent here — actual
    // log output is observed via `cargo test --nocapture` if needed.
    warn_legacy_prefixed_once("maya-anim.set_keyframe");
    warn_legacy_prefixed_once("maya-anim.set_keyframe");
    warn_legacy_prefixed_once("maya-geo.create_sphere");
}
