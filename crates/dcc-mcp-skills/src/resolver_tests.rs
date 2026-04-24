//! Unit tests for the `resolver` module.
#![cfg(test)]

use super::*;

/// Helper to create a minimal SkillMetadata with name and depends.
fn skill(name: &str, depends: &[&str]) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        depends: depends.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

// ── resolve_dependencies tests ──

mod test_resolve_happy_path {
    use super::*;

    #[test]
    fn empty_input() {
        let result = resolve_dependencies(&[]).unwrap();
        assert!(result.ordered.is_empty());
    }

    #[test]
    fn single_skill_no_deps() {
        let skills = [skill("a", &[])];
        let result = resolve_dependencies(&skills).unwrap();
        assert_eq!(result.ordered.len(), 1);
        assert_eq!(result.ordered[0].name, "a");
    }

    #[test]
    fn linear_chain() {
        // c depends on b, b depends on a => order: a, b, c
        let skills = [skill("c", &["b"]), skill("b", &["a"]), skill("a", &[])];
        let result = resolve_dependencies(&skills).unwrap();
        let names: Vec<&str> = result.ordered.iter().map(|s| s.name.as_str()).collect();
        let a_pos = names.iter().position(|&n| n == "a").unwrap();
        let b_pos = names.iter().position(|&n| n == "b").unwrap();
        let c_pos = names.iter().position(|&n| n == "c").unwrap();
        assert!(a_pos < b_pos, "a must come before b");
        assert!(b_pos < c_pos, "b must come before c");
    }

    #[test]
    fn diamond_dependency() {
        // d depends on b,c; b depends on a; c depends on a
        let skills = [
            skill("a", &[]),
            skill("b", &["a"]),
            skill("c", &["a"]),
            skill("d", &["b", "c"]),
        ];
        let result = resolve_dependencies(&skills).unwrap();
        let names: Vec<&str> = result.ordered.iter().map(|s| s.name.as_str()).collect();
        let a_pos = names.iter().position(|&n| n == "a").unwrap();
        let b_pos = names.iter().position(|&n| n == "b").unwrap();
        let c_pos = names.iter().position(|&n| n == "c").unwrap();
        let d_pos = names.iter().position(|&n| n == "d").unwrap();
        assert!(a_pos < b_pos);
        assert!(a_pos < c_pos);
        assert!(b_pos < d_pos);
        assert!(c_pos < d_pos);
    }

    #[test]
    fn multiple_independent_skills() {
        let skills = [skill("a", &[]), skill("b", &[]), skill("c", &[])];
        let result = resolve_dependencies(&skills).unwrap();
        assert_eq!(result.ordered.len(), 3);
    }

    #[test]
    fn complex_graph() {
        // f depends on d,e; d depends on b,c; e depends on c; b depends on a; c depends on a
        let skills = [
            skill("a", &[]),
            skill("b", &["a"]),
            skill("c", &["a"]),
            skill("d", &["b", "c"]),
            skill("e", &["c"]),
            skill("f", &["d", "e"]),
        ];
        let result = resolve_dependencies(&skills).unwrap();
        let names: Vec<&str> = result.ordered.iter().map(|s| s.name.as_str()).collect();
        // Verify all ordering constraints
        let pos = |name: &str| names.iter().position(|&n| n == name).unwrap();
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));
        assert!(pos("c") < pos("e"));
        assert!(pos("d") < pos("f"));
        assert!(pos("e") < pos("f"));
    }

    #[test]
    fn preserves_metadata() {
        let mut s = skill("test", &[]);
        s.dcc = "maya".to_string();
        s.version = "2.0.0".to_string();
        s.description = "Test skill".to_string();
        let result = resolve_dependencies(&[s]).unwrap();
        assert_eq!(result.ordered[0].dcc, "maya");
        assert_eq!(result.ordered[0].version, "2.0.0");
        assert_eq!(result.ordered[0].description, "Test skill");
    }
}

mod test_resolve_error_path {
    use super::*;

    #[test]
    fn missing_dependency() {
        let skills = [skill("a", &["nonexistent"])];
        let err = resolve_dependencies(&skills).unwrap_err();
        match err {
            ResolveError::MissingDependency { skill, dependency } => {
                assert_eq!(skill, "a");
                assert_eq!(dependency, "nonexistent");
            }
            other => panic!("Expected MissingDependency, got: {other:?}"),
        }
    }

    #[test]
    fn direct_cycle() {
        // a depends on b, b depends on a
        let skills = [skill("a", &["b"]), skill("b", &["a"])];
        let err = resolve_dependencies(&skills).unwrap_err();
        match err {
            ResolveError::CyclicDependency { cycle } => {
                assert!(cycle.len() >= 2, "Cycle path too short: {cycle:?}");
                // The cycle should contain both a and b
                assert!(cycle.contains(&"a".to_string()));
                assert!(cycle.contains(&"b".to_string()));
            }
            other => panic!("Expected CyclicDependency, got: {other:?}"),
        }
    }

    #[test]
    fn self_dependency() {
        let skills = [skill("a", &["a"])];
        let err = resolve_dependencies(&skills).unwrap_err();
        match err {
            ResolveError::CyclicDependency { cycle } => {
                assert!(cycle.contains(&"a".to_string()));
            }
            other => panic!("Expected CyclicDependency, got: {other:?}"),
        }
    }

    #[test]
    fn indirect_cycle() {
        // a → b → c → a
        let skills = [skill("a", &["b"]), skill("b", &["c"]), skill("c", &["a"])];
        let err = resolve_dependencies(&skills).unwrap_err();
        match err {
            ResolveError::CyclicDependency { cycle } => {
                assert!(cycle.len() >= 3, "Cycle path too short: {cycle:?}");
            }
            other => panic!("Expected CyclicDependency, got: {other:?}"),
        }
    }

    #[test]
    fn cycle_with_non_cyclic_nodes() {
        // a has no deps, b → c → b (cycle), d depends on a
        let skills = [
            skill("a", &[]),
            skill("b", &["c"]),
            skill("c", &["b"]),
            skill("d", &["a"]),
        ];
        let err = resolve_dependencies(&skills).unwrap_err();
        assert!(matches!(err, ResolveError::CyclicDependency { .. }));
    }
}

// ── validate_dependencies tests ──

mod test_validate {
    use super::*;

    #[test]
    fn valid_dependencies() {
        let skills = [skill("a", &[]), skill("b", &["a"]), skill("c", &["a", "b"])];
        let errors = validate_dependencies(&skills);
        assert!(errors.is_empty());
    }

    #[test]
    fn no_dependencies() {
        let skills = [skill("a", &[]), skill("b", &[])];
        let errors = validate_dependencies(&skills);
        assert!(errors.is_empty());
    }

    #[test]
    fn multiple_missing_dependencies() {
        let skills = [skill("a", &["x", "y"]), skill("b", &["z"])];
        let errors = validate_dependencies(&skills);
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn empty_input() {
        let errors = validate_dependencies(&[]);
        assert!(errors.is_empty());
    }

    #[test]
    fn partial_missing() {
        let skills = [skill("a", &[]), skill("b", &["a", "missing"])];
        let errors = validate_dependencies(&skills);
        assert_eq!(errors.len(), 1);
        match &errors[0] {
            ResolveError::MissingDependency { skill, dependency } => {
                assert_eq!(skill, "b");
                assert_eq!(dependency, "missing");
            }
            other => panic!("Expected MissingDependency, got: {other:?}"),
        }
    }
}

// ── expand_transitive_dependencies tests ──

mod test_expand_transitive {
    use super::*;

    #[test]
    fn no_dependencies() {
        let skills = [skill("a", &[])];
        let result = expand_transitive_dependencies(&skills, "a").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn direct_dependency() {
        let skills = [skill("a", &[]), skill("b", &["a"])];
        let result = expand_transitive_dependencies(&skills, "b").unwrap();
        assert_eq!(result, vec!["a"]);
    }

    #[test]
    fn transitive_chain() {
        let skills = [skill("a", &[]), skill("b", &["a"]), skill("c", &["b"])];
        let result = expand_transitive_dependencies(&skills, "c").unwrap();
        assert!(result.contains(&"a".to_string()));
        assert!(result.contains(&"b".to_string()));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn diamond_transitive() {
        let skills = [
            skill("a", &[]),
            skill("b", &["a"]),
            skill("c", &["a"]),
            skill("d", &["b", "c"]),
        ];
        let result = expand_transitive_dependencies(&skills, "d").unwrap();
        assert!(result.contains(&"a".to_string()));
        assert!(result.contains(&"b".to_string()));
        assert!(result.contains(&"c".to_string()));
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn nonexistent_root_skill() {
        let skills = [skill("a", &[])];
        let result = expand_transitive_dependencies(&skills, "nonexistent").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn missing_transitive_dependency() {
        let skills = [skill("a", &["missing"]), skill("b", &["a"])];
        let err = expand_transitive_dependencies(&skills, "b").unwrap_err();
        assert!(matches!(err, ResolveError::MissingDependency { .. }));
    }

    #[test]
    fn cycle_detected() {
        let skills = [skill("a", &["b"]), skill("b", &["a"])];
        let err = expand_transitive_dependencies(&skills, "a").unwrap_err();
        assert!(matches!(err, ResolveError::CyclicDependency { .. }));
    }
}

// ── Error Display tests ──

mod test_error_display {
    use super::*;

    #[test]
    fn missing_dependency_display() {
        let err = ResolveError::MissingDependency {
            skill: "pipeline".to_string(),
            dependency: "geometry".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("pipeline"));
        assert!(msg.contains("geometry"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn cyclic_dependency_display() {
        let err = ResolveError::CyclicDependency {
            cycle: vec!["a".to_string(), "b".to_string(), "a".to_string()],
        };
        let msg = err.to_string();
        assert!(msg.contains("a → b → a"));
    }
}
