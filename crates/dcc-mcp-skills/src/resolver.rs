//! Skill dependency resolver — topological sort, cycle detection, and validation.
//!
//! Given a set of [`SkillMetadata`] entries, the resolver can:
//!
//! 1. **Validate** that all declared dependencies exist in the provided set.
//! 2. **Detect cycles** in the dependency graph (A → B → C → A).
//! 3. **Topologically sort** skills so that every skill appears after its dependencies.
//! 4. **Expand transitive dependencies** for a single skill (all skills it transitively needs).

use std::collections::{HashMap, HashSet, VecDeque};

use dcc_mcp_models::SkillMetadata;

// ── Error types ──

/// Errors that can occur during dependency resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// A skill declares a dependency on a skill that does not exist in the set.
    MissingDependency {
        /// The skill that declared the dependency.
        skill: String,
        /// The dependency name that was not found.
        dependency: String,
    },
    /// A circular dependency was detected in the graph.
    CyclicDependency {
        /// The cycle path, e.g. `["A", "B", "C", "A"]`.
        cycle: Vec<String>,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingDependency { skill, dependency } => {
                write!(
                    f,
                    "Skill '{skill}' depends on '{dependency}', but it was not found. \
                     Ensure '{dependency}' is available in one of the skill search paths."
                )
            }
            Self::CyclicDependency { cycle } => {
                write!(f, "Circular dependency detected: {}", cycle.join(" → "))
            }
        }
    }
}

impl std::error::Error for ResolveError {}

// ── Resolution result ──

/// The result of a successful dependency resolution.
#[derive(Debug, Clone)]
pub struct ResolvedSkills {
    /// Skills in topological order (dependencies come first).
    pub ordered: Vec<SkillMetadata>,
}

// ── Core resolver ──

/// Resolve skill dependencies using Kahn's algorithm (BFS topological sort).
///
/// # Errors
///
/// Returns [`ResolveError::MissingDependency`] if any skill references a dependency
/// that is not present in the input set.
///
/// Returns [`ResolveError::CyclicDependency`] if the dependency graph contains a cycle.
pub fn resolve_dependencies(skills: &[SkillMetadata]) -> Result<ResolvedSkills, ResolveError> {
    if skills.is_empty() {
        return Ok(ResolvedSkills {
            ordered: Vec::new(),
        });
    }

    // Build name → index map
    let name_to_idx: HashMap<&str, usize> = skills
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.as_str(), i))
        .collect();

    // Validate: every declared dependency must exist
    for skill in skills {
        for dep in &skill.depends {
            if !name_to_idx.contains_key(dep.as_str()) {
                return Err(ResolveError::MissingDependency {
                    skill: skill.name.clone(),
                    dependency: dep.clone(),
                });
            }
        }
    }

    // Build adjacency list and in-degree counts
    // Edge: dependency → dependent (dep must come before dependent)
    let n = skills.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree: Vec<usize> = vec![0; n];

    for (i, skill) in skills.iter().enumerate() {
        for dep in &skill.depends {
            let dep_idx = name_to_idx[dep.as_str()];
            adj[dep_idx].push(i);
            in_degree[i] += 1;
        }
    }

    // Kahn's algorithm
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut ordered_indices: Vec<usize> = Vec::with_capacity(n);
    while let Some(idx) = queue.pop_front() {
        ordered_indices.push(idx);
        for &neighbor in &adj[idx] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    // If we didn't process all nodes, there is a cycle
    if ordered_indices.len() != n {
        let cycle = find_cycle(skills, &name_to_idx);
        return Err(ResolveError::CyclicDependency { cycle });
    }

    let ordered = ordered_indices
        .into_iter()
        .map(|i| skills[i].clone())
        .collect();

    Ok(ResolvedSkills { ordered })
}

/// Validate that all dependencies exist without performing a full sort.
///
/// Returns a list of all missing dependency errors found.
pub fn validate_dependencies(skills: &[SkillMetadata]) -> Vec<ResolveError> {
    let names: HashSet<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    let mut errors = Vec::new();

    for skill in skills {
        for dep in &skill.depends {
            if !names.contains(dep.as_str()) {
                errors.push(ResolveError::MissingDependency {
                    skill: skill.name.clone(),
                    dependency: dep.clone(),
                });
            }
        }
    }
    errors
}

/// Expand all transitive dependencies for a skill by name.
///
/// Returns the full set of skill names that `skill_name` transitively depends on
/// (not including `skill_name` itself), in no particular order.
///
/// # Errors
///
/// Returns [`ResolveError::MissingDependency`] if a referenced dependency is missing.
/// Returns [`ResolveError::CyclicDependency`] if a cycle is detected during traversal.
pub fn expand_transitive_dependencies(
    skills: &[SkillMetadata],
    skill_name: &str,
) -> Result<Vec<String>, ResolveError> {
    let name_to_skill: HashMap<&str, &SkillMetadata> =
        skills.iter().map(|s| (s.name.as_str(), s)).collect();

    let mut visited: HashSet<String> = HashSet::new();
    let mut path: Vec<String> = Vec::new();
    let mut result: Vec<String> = Vec::new();

    expand_dfs(
        skill_name,
        &name_to_skill,
        &mut visited,
        &mut path,
        &mut result,
    )?;

    Ok(result)
}

/// DFS helper for transitive dependency expansion.
fn expand_dfs(
    skill_name: &str,
    name_to_skill: &HashMap<&str, &SkillMetadata>,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
    result: &mut Vec<String>,
) -> Result<(), ResolveError> {
    if visited.contains(skill_name) {
        return Ok(());
    }

    // Cycle detection: if skill_name is already in the current DFS path
    if let Some(pos) = path.iter().position(|p| p == skill_name) {
        let mut cycle: Vec<String> = path[pos..].to_vec();
        cycle.push(skill_name.to_string());
        return Err(ResolveError::CyclicDependency { cycle });
    }

    let skill = match name_to_skill.get(skill_name) {
        Some(s) => s,
        None => {
            // If this is the root skill being queried, it might not exist
            // Only report missing if it was referenced as a dependency
            if !path.is_empty() {
                return Err(ResolveError::MissingDependency {
                    skill: path.last().unwrap().clone(),
                    dependency: skill_name.to_string(),
                });
            }
            return Ok(());
        }
    };

    path.push(skill_name.to_string());

    for dep in &skill.depends {
        if !name_to_skill.contains_key(dep.as_str()) {
            return Err(ResolveError::MissingDependency {
                skill: skill_name.to_string(),
                dependency: dep.clone(),
            });
        }
        expand_dfs(dep, name_to_skill, visited, path, result)?;
        if !result.contains(dep) {
            result.push(dep.clone());
        }
    }

    path.pop();
    visited.insert(skill_name.to_string());

    Ok(())
}

// ── Cycle finder (for error reporting) ──

/// Find a cycle in the dependency graph for detailed error reporting.
///
/// Uses DFS with coloring (white/gray/black) to find a cycle path.
fn find_cycle(skills: &[SkillMetadata], name_to_idx: &HashMap<&str, usize>) -> Vec<String> {
    let n = skills.len();

    // 0 = white (unvisited), 1 = gray (in stack), 2 = black (done)
    let mut color: Vec<u8> = vec![0; n];
    let mut parent: Vec<Option<usize>> = vec![None; n];

    for start in 0..n {
        if color[start] != 0 {
            continue;
        }
        if let Some(cycle) = dfs_find_cycle(start, skills, name_to_idx, &mut color, &mut parent) {
            return cycle;
        }
    }

    // Fallback: list all nodes still in the cycle (in_degree > 0 after Kahn's)
    skills
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.depends.is_empty())
        .map(|(_, s)| s.name.clone())
        .collect()
}

/// DFS coloring to find a cycle path.
fn dfs_find_cycle(
    node: usize,
    skills: &[SkillMetadata],
    name_to_idx: &HashMap<&str, usize>,
    color: &mut [u8],
    parent: &mut [Option<usize>],
) -> Option<Vec<String>> {
    color[node] = 1; // gray

    for dep_name in &skills[node].depends {
        if let Some(&dep_idx) = name_to_idx.get(dep_name.as_str()) {
            if color[dep_idx] == 1 {
                // Found cycle: backtrack from node to dep_idx
                let mut cycle = vec![skills[dep_idx].name.clone()];
                let mut cur = node;
                while cur != dep_idx {
                    cycle.push(skills[cur].name.clone());
                    cur = parent[cur].unwrap_or(dep_idx);
                }
                cycle.push(skills[dep_idx].name.clone());
                cycle.reverse();
                return Some(cycle);
            }
            if color[dep_idx] == 0 {
                parent[dep_idx] = Some(node);
                if let Some(cycle) = dfs_find_cycle(dep_idx, skills, name_to_idx, color, parent) {
                    return Some(cycle);
                }
            }
        }
    }

    color[node] = 2; // black
    None
}

// ── Python bindings ──

/// Python wrapper for resolve_dependencies.
///
/// Returns a list of SkillMetadata in dependency order.
/// Raises ValueError on missing deps or cycles.
#[cfg(feature = "python-bindings")]
#[pyo3::prelude::pyfunction]
#[pyo3(name = "resolve_dependencies")]
pub fn py_resolve_dependencies(
    skills: Vec<dcc_mcp_models::SkillMetadata>,
) -> pyo3::PyResult<Vec<dcc_mcp_models::SkillMetadata>> {
    resolve_dependencies(&skills)
        .map(|r| r.ordered)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
}

/// Python wrapper for validate_dependencies.
///
/// Returns a list of error message strings for all missing dependencies.
#[cfg(feature = "python-bindings")]
#[pyo3::prelude::pyfunction]
#[pyo3(name = "validate_dependencies")]
pub fn py_validate_dependencies(skills: Vec<dcc_mcp_models::SkillMetadata>) -> Vec<String> {
    validate_dependencies(&skills)
        .into_iter()
        .map(|e| e.to_string())
        .collect()
}

/// Python wrapper for expand_transitive_dependencies.
///
/// Returns a list of skill names that `skill_name` transitively depends on.
/// Raises ValueError on missing deps or cycles.
#[cfg(feature = "python-bindings")]
#[pyo3::prelude::pyfunction]
#[pyo3(name = "expand_transitive_dependencies")]
pub fn py_expand_transitive_dependencies(
    skills: Vec<dcc_mcp_models::SkillMetadata>,
    skill_name: &str,
) -> pyo3::PyResult<Vec<String>> {
    expand_transitive_dependencies(&skills, skill_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
}

// ── Tests ──

#[cfg(test)]
mod tests {
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
}
