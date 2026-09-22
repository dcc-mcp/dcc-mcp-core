//! Real skill seeds harvested from this repository (PIP-3408).
//!
//! Synthetic corpora alone only measure how well the scorer separates
//! `maya-skill-00001` from `blender-skill-00002` — the DCC prefix does all the
//! work and the numbers come out flattering. The benchmark therefore starts
//! from the skills this repository actually ships and only *fills* up to the
//! target corpus size with synthetic rows.
//!
//! Seeds are harvested with the production loader
//! ([`dcc_mcp_skills::parse_skill_md`]), so a seed carries the same fields the
//! ranker sees at runtime — frontmatter plus sibling `tools.yaml` entries.
//!
//! The harvest is written to `benchmarks/skills/seeds.json` and committed, so
//! a benchmark run never depends on the state of a developer's working tree:
//!
//! ```text
//! cargo run -p dcc-mcp-skills-bench --bin skills-bench -- regenerate-seeds
//! ```

use std::path::{Path, PathBuf};

use dcc_mcp_models::SkillMetadata;
use serde::{Deserialize, Serialize};

/// Skill directories in this repository that act as real corpus seeds.
///
/// Paths are relative to the workspace root. `tests/fixtures` is included
/// deliberately: those skills exist to exercise the loader's edge cases, which
/// makes them useful ranking stress cases too.
pub const SEED_ROOTS: [&str; 4] = [
    "skills",
    "examples/skills",
    "python/dcc_mcp_core/skills",
    "tests/fixtures/skills",
];

/// Committed seed snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedSet {
    /// Schema tag so a stale snapshot is detected instead of silently used.
    pub schema: String,
    /// Workspace-relative roots the snapshot was harvested from.
    pub roots: Vec<String>,
    /// Harvested skills, sorted by name for a stable diff.
    pub skills: Vec<SkillMetadata>,
}

/// Workspace root of this crate (`<root>/crates/dcc-mcp-skills-bench`).
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Location of the committed seed snapshot.
#[must_use]
pub fn dataset_dir() -> PathBuf {
    workspace_root().join("benchmarks").join("skills")
}

/// Path of the committed seed snapshot.
#[must_use]
pub fn seeds_path() -> PathBuf {
    dataset_dir().join("seeds.json")
}

fn skill_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Harvest real skills from [`SEED_ROOTS`] under `root`.
///
/// Directories the production loader rejects are skipped, not fatal: the seed
/// set is a best-effort snapshot of what the repository ships.
///
/// Absolute paths are rewritten to workspace-relative ones before the skills
/// are stored. The loader fills in `skill_path` and `scripts` with real
/// filesystem paths, so without this the committed snapshot would carry the
/// machine that generated it (`P:\\monica\\...` or `/home/runner/...`) and
/// could never be reproduced anywhere else.
#[must_use]
pub fn harvest(root: &Path) -> Vec<SkillMetadata> {
    let mut skills = Vec::new();
    for relative in SEED_ROOTS {
        let dir = root.join(relative);
        for skill_dir in skill_dirs(&dir) {
            if let Some(metadata) = dcc_mcp_skills::parse_skill_md(&skill_dir) {
                skills.push(relativise(metadata, root));
            }
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills.dedup_by(|a, b| a.name == b.name);
    skills
}

/// Rewrite every absolute path under `root` to a workspace-relative one.
fn relativise(mut skill: SkillMetadata, root: &Path) -> SkillMetadata {
    skill.skill_path = relativise_path(&skill.skill_path, root);
    skill.scripts = skill
        .scripts
        .iter()
        .map(|script| relativise_path(script, root))
        .collect();
    skill
}

/// Strip the `root` prefix, keeping a `/`-separated relative path.
///
/// Paths that are already relative, or that fall outside the workspace, are
/// left alone — the goal is reproducibility, not normalisation for its own
/// sake.
fn relativise_path(path: &str, root: &Path) -> String {
    let candidate = Path::new(path);
    let stripped = candidate.strip_prefix(root).unwrap_or(candidate);
    stripped
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Build the snapshot for `root`.
#[must_use]
pub fn build_seed_set(root: &Path) -> SeedSet {
    SeedSet {
        schema: crate::synthetic::CORPUS_SCHEMA_VERSION.to_string(),
        roots: SEED_ROOTS.iter().map(|r| (*r).to_string()).collect(),
        skills: harvest(root),
    }
}

/// Load the committed snapshot, returning `None` when it is absent.
///
/// A snapshot whose `schema` does not match the current generator is treated
/// as absent: the caller regenerates rather than benchmarking a stale corpus.
#[must_use]
pub fn load_committed() -> Option<SeedSet> {
    let text = std::fs::read_to_string(seeds_path()).ok()?;
    let set: SeedSet = serde_json::from_str(&text).ok()?;
    (set.schema == crate::synthetic::CORPUS_SCHEMA_VERSION).then_some(set)
}

/// Write `set` to the committed snapshot path.
pub fn write_committed(set: &SeedSet) -> std::io::Result<()> {
    let path = seeds_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(set).map_err(std::io::Error::other)?;
    text.push('\n');
    std::fs::write(path, text)
}

/// Seeds to build a corpus from: the committed snapshot when it exists and is
/// current, otherwise a live harvest of the working tree.
#[must_use]
pub fn seeds() -> Vec<SkillMetadata> {
    match load_committed() {
        Some(set) => set.skills,
        None => harvest(&workspace_root()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harvest_finds_the_shipped_skills() {
        let skills = harvest(&workspace_root());
        assert!(
            skills.len() >= 10,
            "expected a meaningful real seed pool, got {}",
            skills.len()
        );
        for skill in &skills {
            assert!(!skill.name.is_empty());
            assert!(
                !skill.description.is_empty(),
                "{} has no description",
                skill.name
            );
        }
    }

    #[test]
    fn harvested_paths_are_workspace_relative() {
        // The snapshot is committed, so it must not carry the machine that
        // produced it. Absolute paths would make it unreproducible on every
        // other checkout and would leak local directory layout.
        let root = workspace_root();
        for skill in harvest(&root) {
            for path in std::iter::once(&skill.skill_path).chain(skill.scripts.iter()) {
                assert!(
                    !Path::new(path).is_absolute(),
                    "{}: absolute path left in the snapshot: {path}",
                    skill.name
                );
            }
        }
    }

    #[test]
    fn committed_snapshot_matches_a_live_harvest() {
        let committed = load_committed().expect("benchmarks/skills/seeds.json must be committed");
        let live = build_seed_set(&workspace_root());
        assert_eq!(
            committed.skills, live.skills,
            "seeds.json is stale — rerun `skills-bench regenerate-seeds`"
        );
    }
}
