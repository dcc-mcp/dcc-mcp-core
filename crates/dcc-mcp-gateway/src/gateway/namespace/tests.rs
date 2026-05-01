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

// ── #656 Cursor-safe encoding ────────────────────────────────────────────
//
// Locks down every client-side regex the `i_<id8>__<escaped>` form has to
// clear, plus the round-trip / collision / error-path guarantees the
// decoder depends on for safe routing.

#[test]
fn cursor_safe_encode_produces_only_alnum_and_underscore() {
    // Cursor's tool-name regex is stricter than SEP-986 (`[A-Za-z0-9_]+`).
    // Every input that survives SEP-986 must come out clean here — this
    // test is the contract the gateway signs with the client.
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    for tool in [
        "create_sphere",               // plain identifier
        "maya-animation.set_keyframe", // skill-prefixed with dot + hyphen
        "CamelCase",                   // mixed case
        "scene.object.transform",      // multiple dots
        "hello-world-greeting",        // multiple hyphens
        "a",                           // single byte
        "0",                           // digit-only leading byte
        "v2.dotted.chain_name",        // dots and underscores together
    ] {
        let enc = encode_tool_name_cursor_safe(&id, tool);
        assert!(
            is_cursor_safe_alphabet(&enc),
            "cursor-safe encoding of {tool:?} yielded {enc:?} with disallowed chars",
        );
        assert!(
            !enc.contains('.') && !enc.contains('-') && !enc.contains('/'),
            "cursor-safe encoding of {tool:?} yielded {enc:?} which still contains a forbidden separator",
        );
        assert!(
            validate_tool_name(&enc).is_ok(),
            "cursor-safe encoding of {tool:?} yielded {enc:?} which fails SEP-986",
        );
        assert!(
            enc.starts_with("i_abcdef01__"),
            "cursor-safe encoding must carry the instance prefix verbatim: got {enc:?}",
        );
    }
}

#[test]
fn cursor_safe_encode_escape_vocabulary_is_exhaustive() {
    // Pin the escape table so a future refactor cannot silently drop or
    // rename a mapping.
    assert_eq!(escape_cursor_safe("a.b"), "a_D_b");
    assert_eq!(escape_cursor_safe("a-b"), "a_H_b");
    assert_eq!(escape_cursor_safe("a_b"), "a_U_b");
    assert_eq!(escape_cursor_safe("plain"), "plain");
    assert_eq!(escape_cursor_safe(""), "");
    // Back-to-back specials must not merge or collide.
    assert_eq!(escape_cursor_safe("._-"), "_D__U__H_");
}

#[test]
fn cursor_safe_decode_is_inverse_of_encode_for_every_valid_backend_name() {
    // Every backend tool name that passes SEP-986 validation must
    // round-trip losslessly through cursor-safe encoding. This keeps
    // the gateway from quietly renaming tools on its way to Cursor.
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    for tool in [
        "create_sphere",
        "maya-animation.set_keyframe",
        "scene.object.transform",
        "CamelCase",
        "x",
        "hello-world",
        "dotted.name.with_underscore",
        "with.dot-and-dash",
    ] {
        let enc = encode_tool_name_cursor_safe(&id, tool);
        let (p, o) = decode_tool_name(&enc)
            .unwrap_or_else(|| panic!("decode_tool_name lost cursor-safe name {enc:?}"));
        assert_eq!(p, "abcdef01");
        assert_eq!(o, tool, "round-trip of {tool:?} via {enc:?} dropped bytes");
    }
}

#[test]
fn cursor_safe_decode_accepts_backend_tool_names_with_dots() {
    // The spec explicitly requires support for backend names that
    // themselves contain dots. If this regresses, any Maya skill-prefixed
    // action (e.g. `maya-animation.set_keyframe`) becomes unreachable
    // through Cursor.
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    let enc = encode_tool_name_cursor_safe(&id, "maya-animation.set_keyframe");
    assert_eq!(enc, "i_abcdef01__maya_H_animation_D_set_U_keyframe");
    let (p, o) = decode_tool_name(&enc).unwrap();
    assert_eq!(p, "abcdef01");
    assert_eq!(o, "maya-animation.set_keyframe");
}

#[test]
fn cursor_safe_decode_rejects_malformed_escape_sequences() {
    // A lone `_` never appears in a well-formed cursor-safe payload;
    // decoding one must fail rather than silently produce a corrupted
    // backend name that would then be routed to the wrong tool.
    assert!(unescape_cursor_safe("abc_").is_none());
    assert!(unescape_cursor_safe("abc_Z_").is_none()); // unknown escape
    assert!(unescape_cursor_safe("abc_D").is_none()); // truncated
    assert!(unescape_cursor_safe("abc_Dx").is_none()); // wrong terminator
    // But valid escapes still round-trip:
    assert_eq!(unescape_cursor_safe("abc_D_").unwrap(), "abc.");
}

#[test]
fn cursor_safe_decode_falls_through_on_bad_payload() {
    // `i_abcdef01__` prefix matches the shape, but the payload `bad_`
    // is not a valid escape sequence. The decoder must NOT route this
    // as a cursor-safe name (that would silently drop the `_`) — it
    // either falls through to legacy arms or returns None. Either way,
    // the caller sees no phantom tool.
    let decoded = decode_tool_name("i_abcdef01__bad_");
    assert!(
        decoded.is_none(),
        "malformed cursor-safe payload must not decode as cursor-safe: got {decoded:?}",
    );
}

#[test]
fn decode_prefers_cursor_safe_over_legacy_underscore() {
    // Both arms split on `__`, so order matters. A cursor-safe name
    // `i_abcdef01__foo` must NEVER be routed as a pre-#258 legacy name
    // `(i, abcdef01__foo)` — the `i_` prefix guards the cursor-safe
    // arm, so legacy prefixes that happen to be the byte `i` followed
    // by an 8-hex-ish chunk don't count as legacy.
    let (p, o) = decode_tool_name("i_abcdef01__create_U_sphere").unwrap();
    assert_eq!(p, "abcdef01");
    assert_eq!(o, "create_sphere");
}

#[test]
fn cursor_safe_pre_656_dot_form_still_decodes_during_compat_window() {
    // Until every client has rolled over, the old SEP-986 dot form
    // must keep working so agents that haven't upgraded don't see
    // Unknown tool errors.
    let (p, o) = decode_tool_name("abcdef01.create_sphere").unwrap();
    assert_eq!(p, "abcdef01");
    assert_eq!(o, "create_sphere");
}

#[test]
fn cursor_safe_encode_length_budget_fits_in_48() {
    // MAX_TOOL_NAME_LEN is 48 in dcc-mcp-naming. The `i_<id8>__`
    // prefix is 12 bytes, leaving 36 bytes for the escaped payload.
    // The worst-case expansion ratio is 3x (every input byte becomes
    // `_?_`), so a backend name ≤ 12 bytes is guaranteed to fit. Pin
    // that budget explicitly so future tool-name caps (or prefix
    // growth) notice the regression here rather than at runtime.
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    // 12-byte name entirely made of dots → 36-byte payload → 48 total.
    let worst = ".".repeat(12);
    let enc = encode_tool_name_cursor_safe(&id, &worst);
    assert_eq!(enc.len(), 48);
    assert!(validate_tool_name(&enc).is_ok());
}

#[test]
fn is_cursor_safe_alphabet_matches_cursor_regex() {
    assert!(is_cursor_safe_alphabet("create_sphere"));
    assert!(is_cursor_safe_alphabet("i_abcdef01__foo"));
    assert!(is_cursor_safe_alphabet("A"));
    // `.` `-` `/` and whitespace are all rejected.
    assert!(!is_cursor_safe_alphabet("tool.name"));
    assert!(!is_cursor_safe_alphabet("tool-name"));
    assert!(!is_cursor_safe_alphabet("tool/name"));
    assert!(!is_cursor_safe_alphabet("tool name"));
    assert!(!is_cursor_safe_alphabet(""));
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
