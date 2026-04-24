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

#[cfg(test)]
#[path = "resolver_tests.rs"]
mod tests;
