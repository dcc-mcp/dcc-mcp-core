use super::*;

// ── SemVer parsing ───────────────────────────────────────────────────────────

mod test_semver_parse {
    use super::*;

    #[test]
    fn test_parse_full_triple() {
        let v = SemVer::parse("1.2.3").unwrap();
        assert_eq!(v, SemVer::new(1, 2, 3));
    }

    #[test]
    fn test_parse_with_v_prefix() {
        let v = SemVer::parse("v2.0.0").unwrap();
        assert_eq!(v, SemVer::new(2, 0, 0));
    }

    #[test]
    fn test_parse_two_components() {
        let v = SemVer::parse("3.1").unwrap();
        assert_eq!(v, SemVer::new(3, 1, 0));
    }

    #[test]
    fn test_parse_one_component() {
        let v = SemVer::parse("4").unwrap();
        assert_eq!(v, SemVer::new(4, 0, 0));
    }

    #[test]
    fn test_parse_strips_prerelease_label() {
        let v = SemVer::parse("1.0.0-alpha").unwrap();
        assert_eq!(v, SemVer::new(1, 0, 0));
    }

    #[test]
    fn test_parse_strips_prerelease_complex() {
        let v = SemVer::parse("2.1.0-beta.3").unwrap();
        assert_eq!(v, SemVer::new(2, 1, 0));
    }

    #[test]
    fn test_parse_empty_returns_error() {
        assert_eq!(SemVer::parse(""), Err(VersionParseError::EmptyString));
    }

    #[test]
    fn test_parse_invalid_component_returns_error() {
        let err = SemVer::parse("1.x.0").unwrap_err();
        assert!(matches!(err, VersionParseError::InvalidComponent(_)));
    }

    #[test]
    fn test_ordering_major() {
        assert!(SemVer::new(2, 0, 0) > SemVer::new(1, 9, 9));
    }

    #[test]
    fn test_ordering_minor() {
        assert!(SemVer::new(1, 3, 0) > SemVer::new(1, 2, 99));
    }

    #[test]
    fn test_ordering_patch() {
        assert!(SemVer::new(1, 0, 5) > SemVer::new(1, 0, 4));
    }

    #[test]
    fn test_display() {
        assert_eq!(SemVer::new(1, 2, 3).to_string(), "1.2.3");
    }

    #[test]
    fn test_fromstr_trait() {
        let v: SemVer = "3.14.15".parse().unwrap();
        assert_eq!(v, SemVer::new(3, 14, 15));
    }
}

// ── VersionConstraint parsing & matching ─────────────────────────────────────

mod test_constraints {
    use super::*;

    fn ver(s: &str) -> SemVer {
        SemVer::parse(s).unwrap()
    }

    fn constraint(s: &str) -> VersionConstraint {
        s.parse().unwrap()
    }

    #[test]
    fn test_any_matches_everything() {
        let c = constraint("*");
        assert!(c.matches(ver("0.0.1")));
        assert!(c.matches(ver("99.99.99")));
    }

    #[test]
    fn test_exact_matches_only_same() {
        let c = constraint("=1.2.3");
        assert!(c.matches(ver("1.2.3")));
        assert!(!c.matches(ver("1.2.4")));
        assert!(!c.matches(ver("1.2.2")));
    }

    #[test]
    fn test_bare_version_is_exact() {
        let c = constraint("1.2.3");
        assert!(c.matches(ver("1.2.3")));
        assert!(!c.matches(ver("1.2.4")));
    }

    #[test]
    fn test_at_least() {
        let c = constraint(">=1.2.0");
        assert!(c.matches(ver("1.2.0")));
        assert!(c.matches(ver("1.2.1")));
        assert!(c.matches(ver("2.0.0")));
        assert!(!c.matches(ver("1.1.9")));
    }

    #[test]
    fn test_greater_than() {
        let c = constraint(">1.2.0");
        assert!(c.matches(ver("1.2.1")));
        assert!(!c.matches(ver("1.2.0")));
        assert!(!c.matches(ver("1.1.9")));
    }

    #[test]
    fn test_at_most() {
        let c = constraint("<=2.0.0");
        assert!(c.matches(ver("2.0.0")));
        assert!(c.matches(ver("1.9.9")));
        assert!(!c.matches(ver("2.0.1")));
    }

    #[test]
    fn test_less_than() {
        let c = constraint("<2.0.0");
        assert!(c.matches(ver("1.9.9")));
        assert!(!c.matches(ver("2.0.0")));
    }

    #[test]
    fn test_caret_same_major() {
        let c = constraint("^1.2.0");
        assert!(c.matches(ver("1.2.0")));
        assert!(c.matches(ver("1.5.3")));
        assert!(c.matches(ver("1.99.0")));
        assert!(!c.matches(ver("2.0.0")));
        assert!(!c.matches(ver("1.1.9")));
    }

    #[test]
    fn test_caret_major_zero() {
        // ^0.2.0 should only allow same major (0), minor >= 2
        let c = constraint("^0.2.0");
        assert!(c.matches(ver("0.2.0")));
        assert!(c.matches(ver("0.3.0")));
        assert!(!c.matches(ver("1.0.0")));
    }

    #[test]
    fn test_tilde_same_major_minor() {
        let c = constraint("~1.2.3");
        assert!(c.matches(ver("1.2.3")));
        assert!(c.matches(ver("1.2.9")));
        assert!(!c.matches(ver("1.3.0")));
        assert!(!c.matches(ver("2.2.3")));
    }

    #[test]
    fn test_constraint_display_round_trip() {
        for s in [
            "*", "=1.2.3", ">=1.0.0", ">2.0.0", "<=3.0.0", "<1.0.0", "^1.2.3", "~1.2.3",
        ] {
            let c: VersionConstraint = s.parse().unwrap();
            assert_eq!(c.to_string(), s, "round-trip failed for '{s}'");
        }
    }
}

// ── VersionedRegistry ────────────────────────────────────────────────────────

mod test_versioned_registry {
    use super::*;

    fn meta(name: &str, dcc: &str, version: &str) -> ActionMeta {
        ActionMeta {
            name: name.into(),
            dcc: dcc.into(),
            version: version.into(),
            ..Default::default()
        }
    }

    #[test]
    fn test_register_and_versions() {
        let mut vr = VersionedRegistry::new();
        vr.register(meta("action", "maya", "1.0.0"));
        vr.register(meta("action", "maya", "1.2.0"));
        vr.register(meta("action", "maya", "2.0.0"));

        let versions = vr.versions("action", "maya");
        assert_eq!(
            versions,
            vec![
                SemVer::new(1, 0, 0),
                SemVer::new(1, 2, 0),
                SemVer::new(2, 0, 0)
            ]
        );
    }

    #[test]
    fn test_register_same_version_overwrites() {
        let mut vr = VersionedRegistry::new();
        let mut m = meta("act", "maya", "1.0.0");
        m.description = "old".into();
        vr.register(m);

        let mut m2 = meta("act", "maya", "1.0.0");
        m2.description = "new".into();
        vr.register(m2);

        let versions = vr.versions("act", "maya");
        assert_eq!(versions.len(), 1);
        assert_eq!(
            vr.get("act", "maya", SemVer::new(1, 0, 0))
                .unwrap()
                .description,
            "new"
        );
    }

    #[test]
    fn test_register_independent_dccs() {
        let mut vr = VersionedRegistry::new();
        vr.register(meta("action", "maya", "1.0.0"));
        vr.register(meta("action", "blender", "2.0.0"));

        assert_eq!(vr.versions("action", "maya"), vec![SemVer::new(1, 0, 0)]);
        assert_eq!(vr.versions("action", "blender"), vec![SemVer::new(2, 0, 0)]);
    }

    #[test]
    fn test_latest_returns_highest() {
        let mut vr = VersionedRegistry::new();
        vr.register(meta("x", "maya", "1.0.0"));
        vr.register(meta("x", "maya", "3.0.0"));
        vr.register(meta("x", "maya", "2.0.0"));

        assert_eq!(vr.latest("x", "maya").unwrap().version, "3.0.0");
    }

    #[test]
    fn test_latest_returns_none_for_unknown() {
        let vr = VersionedRegistry::new();
        assert!(vr.latest("unknown", "maya").is_none());
    }

    #[test]
    fn test_get_specific_version() {
        let mut vr = VersionedRegistry::new();
        vr.register(meta("a", "maya", "1.0.0"));
        vr.register(meta("a", "maya", "2.0.0"));

        assert!(vr.get("a", "maya", SemVer::new(1, 0, 0)).is_some());
        assert!(vr.get("a", "maya", SemVer::new(2, 0, 0)).is_some());
        assert!(vr.get("a", "maya", SemVer::new(3, 0, 0)).is_none());
    }

    #[test]
    fn test_remove_by_constraint() {
        let mut vr = VersionedRegistry::new();
        vr.register(meta("a", "maya", "1.0.0"));
        vr.register(meta("a", "maya", "1.5.0"));
        vr.register(meta("a", "maya", "2.0.0"));

        let constraint: VersionConstraint = "^1.0.0".parse().unwrap();
        let removed = vr.remove("a", "maya", &constraint);
        assert_eq!(removed, 2); // 1.0.0 and 1.5.0 removed
        assert_eq!(vr.versions("a", "maya"), vec![SemVer::new(2, 0, 0)]);
    }

    #[test]
    fn test_total_entries() {
        let mut vr = VersionedRegistry::new();
        vr.register(meta("a", "maya", "1.0.0"));
        vr.register(meta("a", "maya", "2.0.0"));
        vr.register(meta("b", "maya", "1.0.0"));
        assert_eq!(vr.total_entries(), 3);
    }

    #[test]
    fn test_keys_contains_all_pairs() {
        let mut vr = VersionedRegistry::new();
        vr.register(meta("a", "maya", "1.0.0"));
        vr.register(meta("b", "blender", "1.0.0"));

        let mut keys = vr.keys();
        keys.sort();
        assert!(keys.contains(&("a".to_string(), "maya".to_string())));
        assert!(keys.contains(&("b".to_string(), "blender".to_string())));
    }
}

// ── CompatibilityRouter ──────────────────────────────────────────────────────

mod test_router {
    use super::*;

    fn meta(name: &str, dcc: &str, version: &str) -> ActionMeta {
        ActionMeta {
            name: name.into(),
            dcc: dcc.into(),
            version: version.into(),
            description: format!("{version} description"),
            ..Default::default()
        }
    }

    fn registry_with_versions() -> VersionedRegistry {
        let mut vr = VersionedRegistry::new();
        vr.register(meta("create_sphere", "maya", "1.0.0"));
        vr.register(meta("create_sphere", "maya", "1.2.0"));
        vr.register(meta("create_sphere", "maya", "1.5.0"));
        vr.register(meta("create_sphere", "maya", "2.0.0"));
        vr
    }

    #[test]
    fn test_resolve_any_returns_latest() {
        let vr = registry_with_versions();
        let result = vr
            .router()
            .resolve("create_sphere", "maya", &VersionConstraint::Any);
        assert_eq!(result.unwrap().version, "2.0.0");
    }

    #[test]
    fn test_resolve_caret_picks_highest_compatible() {
        let vr = registry_with_versions();
        let c: VersionConstraint = "^1.0.0".parse().unwrap();
        let result = vr.router().resolve("create_sphere", "maya", &c);
        assert_eq!(result.unwrap().version, "1.5.0");
    }

    #[test]
    fn test_resolve_at_least() {
        let vr = registry_with_versions();
        let c: VersionConstraint = ">=1.2.0".parse().unwrap();
        let result = vr.router().resolve("create_sphere", "maya", &c);
        assert_eq!(result.unwrap().version, "2.0.0");
    }

    #[test]
    fn test_resolve_tilde_picks_patch() {
        let mut vr = VersionedRegistry::new();
        vr.register(meta("a", "maya", "1.2.0"));
        vr.register(meta("a", "maya", "1.2.5"));
        vr.register(meta("a", "maya", "1.3.0"));

        let c: VersionConstraint = "~1.2.0".parse().unwrap();
        let result = vr.router().resolve("a", "maya", &c);
        assert_eq!(result.unwrap().version, "1.2.5");
    }

    #[test]
    fn test_resolve_exact() {
        let vr = registry_with_versions();
        let c: VersionConstraint = "=1.2.0".parse().unwrap();
        let result = vr.router().resolve("create_sphere", "maya", &c);
        assert_eq!(result.unwrap().version, "1.2.0");
    }

    #[test]
    fn test_resolve_returns_none_when_no_match() {
        let vr = registry_with_versions();
        let c: VersionConstraint = ">=3.0.0".parse().unwrap();
        assert!(vr.router().resolve("create_sphere", "maya", &c).is_none());
    }

    #[test]
    fn test_resolve_unknown_action_returns_none() {
        let vr = registry_with_versions();
        assert!(
            vr.router()
                .resolve("nonexistent", "maya", &VersionConstraint::Any)
                .is_none()
        );
    }

    #[test]
    fn test_resolve_all_with_caret() {
        let vr = registry_with_versions();
        let c: VersionConstraint = "^1.0.0".parse().unwrap();
        let results = vr.router().resolve_all("create_sphere", "maya", &c);
        let versions: Vec<&str> = results.iter().map(|m| m.version.as_str()).collect();
        assert_eq!(versions, vec!["1.0.0", "1.2.0", "1.5.0"]);
    }

    #[test]
    fn test_resolve_all_none_matching() {
        let vr = registry_with_versions();
        let c: VersionConstraint = ">=10.0.0".parse().unwrap();
        assert!(
            vr.router()
                .resolve_all("create_sphere", "maya", &c)
                .is_empty()
        );
    }

    #[test]
    fn test_resolve_less_than() {
        let vr = registry_with_versions();
        let c: VersionConstraint = "<1.5.0".parse().unwrap();
        let result = vr.router().resolve("create_sphere", "maya", &c);
        // highest below 1.5.0 is 1.2.0
        assert_eq!(result.unwrap().version, "1.2.0");
    }

    #[test]
    fn test_resolve_dcc_isolation() {
        let mut vr = VersionedRegistry::new();
        vr.register(meta("a", "maya", "1.0.0"));
        vr.register(meta("a", "blender", "2.0.0"));

        let c = VersionConstraint::Any;
        assert_eq!(
            vr.router().resolve("a", "maya", &c).unwrap().version,
            "1.0.0"
        );
        assert_eq!(
            vr.router().resolve("a", "blender", &c).unwrap().version,
            "2.0.0"
        );
    }
}
