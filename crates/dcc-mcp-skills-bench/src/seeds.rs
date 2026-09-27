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
///
/// `benchmarks/skills/adapter-skills` is real SKILL.md content vendored from
/// the dcc-mcp organisation's adapter repositories (`PROVENANCE.txt` in that
/// directory records the source of every entry). It is vendored rather than
/// cloned at harvest time so a benchmark run stays reproducible offline and
/// [`crate::seeds::tests::committed_snapshot_matches_a_live_harvest`] compares
/// two things that are both in this repository.
pub const SEED_ROOTS: [&str; 5] = [
    "skills",
    "examples/skills",
    "python/dcc_mcp_core/skills",
    "tests/fixtures/skills",
    ADAPTER_SKILLS_ROOT,
];

/// Seed root holding SKILL.md content vendored from adapter repositories.
///
/// Separated from [`SEED_ROOTS`] because coverage is reported on the skills
/// this repository authors, not on third-party content copied verbatim.
pub const ADAPTER_SKILLS_ROOT: &str = "benchmarks/skills/adapter-skills";

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
///
/// `metadata_files` is easy to miss because only a few skills carry it, which
/// is exactly why it is listed explicitly here rather than discovered.
fn relativise(mut skill: SkillMetadata, root: &Path) -> SkillMetadata {
    skill.skill_path = relativise_path(&skill.skill_path, root);
    skill.scripts = skill
        .scripts
        .iter()
        .map(|script| relativise_path(script, root))
        .collect();
    skill.metadata_files = skill
        .metadata_files
        .iter()
        .map(|file| relativise_path(file, root))
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

    /// Which `RecallContext` fields a skill populates, as a coverage counter.
    #[derive(Debug, Default)]
    struct RecallCoverage {
        total: usize,
        app_type: usize,
        domain: usize,
        workflow_stage: usize,
        task_category: usize,
    }

    impl RecallCoverage {
        fn add(&mut self, skill: &SkillMetadata) {
            self.total += 1;
            let Some(ctx) = skill.recall_context.as_ref() else {
                return;
            };
            self.app_type += usize::from(ctx.app_type.is_some());
            self.domain += usize::from(ctx.domain.is_some());
            self.workflow_stage += usize::from(ctx.workflow_stage.is_some());
            self.task_category += usize::from(ctx.task_category.is_some());
        }

        /// Fraction carrying each field, for the assertion message.
        fn report(&self) -> String {
            let pct = |n: usize| 100.0 * n as f64 / self.total.max(1) as f64;
            format!(
                "app_type {:.1}%, domain {:.1}%, workflow_stage {:.1}%, task_category {:.1}% ({} skills)",
                pct(self.app_type),
                pct(self.domain),
                pct(self.workflow_stage),
                pct(self.task_category),
                self.total
            )
        }
    }

    /// Skills shipped by this repository, i.e. everything except the vendored
    /// adapter corpus.
    fn shipped_skills() -> Vec<SkillMetadata> {
        let root = workspace_root();
        let mut skills = Vec::new();
        for relative in SEED_ROOTS {
            if relative == ADAPTER_SKILLS_ROOT {
                continue;
            }
            for skill_dir in skill_dirs(&root.join(relative)) {
                if let Some(metadata) = dcc_mcp_skills::parse_skill_md(&skill_dir) {
                    skills.push(relativise(metadata, &root));
                }
            }
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        skills.dedup_by(|a, b| a.name == b.name);
        skills
    }

    /// `RecallContext` coverage on the skills this repository ships (PIP-3701).
    ///
    /// The four fields are the structured signal discovery is meant to rank
    /// on. Coverage is measured on shipped skills only: the vendored adapter
    /// corpus is third-party content copied verbatim, so its coverage is a
    /// property of those upstream repos, not something this repository can
    /// raise.
    #[test]
    fn shipped_skills_carry_recall_context() {
        let skills = shipped_skills();
        let mut coverage = RecallCoverage::default();
        for skill in &skills {
            coverage.add(skill);
        }

        assert!(
            coverage.total >= 26,
            "expected the shipped skill set, found {} — did a seed root move?",
            coverage.total
        );
        for (label, count) in [
            ("app_type", coverage.app_type),
            ("domain", coverage.domain),
            ("workflow_stage", coverage.workflow_stage),
            ("task_category", coverage.task_category),
        ] {
            let pct = 100.0 * count as f64 / coverage.total as f64;
            assert!(
                pct >= 90.0,
                "recall_context.{label} coverage {pct:.1}% is below the 90% floor; {}",
                coverage.report()
            );
        }
    }

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
            let paths = std::iter::once(&skill.skill_path)
                .chain(skill.scripts.iter())
                .chain(skill.metadata_files.iter());
            for path in paths {
                assert!(
                    !Path::new(path).is_absolute(),
                    "{}: absolute path left in the snapshot: {path}",
                    skill.name
                );
            }
        }
    }

    /// Staleness check for the committed snapshot.
    ///
    /// `#[ignore]`d on purpose. The snapshot is harvested from four directories
    /// of this repository, so any PR that touches a SKILL.md frontmatter
    /// anywhere in them goes red here even when that PR has nothing to do with
    /// the benchmark. That makes it a poor default gate but a good dedicated
    /// one: `skills-bench.yml` runs it explicitly.
    #[test]
    #[ignore = "whole-repo staleness gate; run from skills-bench.yml, not --workspace"]
    fn committed_snapshot_matches_a_live_harvest() {
        let committed = load_committed().expect("benchmarks/skills/seeds.json must be committed");
        let live = build_seed_set(&workspace_root());
        assert_eq!(
            committed.skills, live.skills,
            "seeds.json is stale. Regenerate: cargo run -p dcc-mcp-skills-bench --bin skills-bench -- regenerate-seeds"
        );
    }
}
