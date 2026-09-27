//! Cross-adapter seed harvest (PIP-3701).
//!
//! [`crate::seeds`] harvests the skills **this repository** ships. That pool
//! is 26 skills, which is not a catalogue: at `SCALE_300` the synthetic
//! generator filled 274 of 300 rows, so the measured hit rate was mostly a
//! measurement of how well the scorer separates synthetic filler from
//! synthetic filler.
//!
//! This module harvests the real skill catalogues of the DCC adapter
//! repositories and commits the result as `benchmarks/skills/adapters.json`.
//! Two properties make that safe for a benchmark that has to be reproducible
//! on a CI machine with no network:
//!
//! * **The harvest is pinned.** Every source carries an explicit commit SHA.
//!   Re-running the harvester against the same SHAs reproduces the same file;
//!   moving to a new SHA is a deliberate, reviewable change.
//! * **The snapshot is the input.** The benchmark reads the committed JSON.
//!   It never clones anything, so a seed set cannot drift between a
//!   developer's machine and CI.
//!
//! # Refreshing
//!
//! ```text
//! cargo run -p dcc-mcp-skills-bench --bin skills-bench -- harvest-adapters
//! ```
//!
//! That clones each pinned commit into `target/adapter-harvest/`, re-parses
//! every skill with the production loader, and rewrites the snapshot. It needs
//! network and `git`; the benchmark itself needs neither.

use std::path::{Path, PathBuf};

use dcc_mcp_models::SkillMetadata;
use serde::{Deserialize, Serialize};

/// One pinned adapter repository the corpus is harvested from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterSource {
    /// `owner/repo` on GitHub.
    pub repo: String,
    /// Commit the snapshot was harvested at.
    pub commit: String,
    /// Repo-relative directories holding one directory per skill.
    pub roots: Vec<String>,
}

impl AdapterSource {
    fn new(repo: &str, commit: &str, roots: Vec<String>) -> Self {
        Self {
            repo: repo.to_string(),
            commit: commit.to_string(),
            roots,
        }
    }

    /// Clone URL.
    #[must_use]
    pub fn url(&self) -> String {
        format!("https://github.com/{}.git", self.repo)
    }

    /// Bare repo name, used to namespace harvested paths.
    #[must_use]
    pub fn name(&self) -> &str {
        self.repo.rsplit('/').next().unwrap_or(&self.repo)
    }
}

/// Adapter repositories the corpus is harvested from, with the commit each
/// snapshot was taken at.
///
/// Seven hosts covering the five synthetic [`crate::synthetic::DCCS`] buckets
/// plus two hosts the synthetic corpus has no bucket for (`3dsmax` is `max`,
/// `photoshop` is not modelled at all), so the real seed pool is not just a
/// re-labelling of the filler's DCC vocabulary.
#[must_use]
pub fn adapter_sources() -> Vec<AdapterSource> {
    vec![
        AdapterSource::new(
            "dcc-mcp/dcc-mcp-maya",
            "02cc398fcf03859953f152f8122668e20214131f",
            vec!["skills".to_string(), "src/dcc_mcp_maya/skills".to_string()],
        ),
        AdapterSource::new(
            "dcc-mcp/dcc-mcp-blender",
            "8e04977ce5bf0258077296e1007a5b11c4cb7aaa",
            vec![
                "skills".to_string(),
                "src/dcc_mcp_blender/skills".to_string(),
            ],
        ),
        AdapterSource::new(
            "dcc-mcp/dcc-mcp-houdini",
            "890d53d984cf871f894a691b82187fa7d44ac698",
            vec![
                "skills".to_string(),
                "src/dcc_mcp_houdini/skills".to_string(),
            ],
        ),
        AdapterSource::new(
            "dcc-mcp/dcc-mcp-3dsmax",
            "76e6954e87b110620297891ad07af7f367a8579b",
            vec![
                "skills".to_string(),
                "src/dcc_mcp_3dsmax/skills".to_string(),
            ],
        ),
        AdapterSource::new(
            "dcc-mcp/dcc-mcp-unreal",
            "7e216a293aed4189cfb518a7c2b1130b84f9f9e6",
            vec!["src/dcc_mcp_unreal/skills".to_string()],
        ),
        AdapterSource::new(
            "dcc-mcp/dcc-mcp-photoshop",
            "49836c11bd121d32658337e76f1c3294a7d84119",
            vec!["src/dcc_mcp_photoshop/skills".to_string()],
        ),
        AdapterSource::new(
            "dcc-mcp/dcc-mcp-speedtree",
            "595f0bd308aa0f4f0635a399c2b63bd76f16caf8",
            vec![
                "skills".to_string(),
                "src/dcc_mcp_speedtree/skills".to_string(),
            ],
        ),
    ]
}

/// Committed cross-adapter seed snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterSeedSet {
    /// Schema tag so a stale snapshot is detected instead of silently used.
    pub schema: String,
    /// Sources the snapshot was harvested from, with their commits.
    pub sources: Vec<AdapterSource>,
    /// Harvested skills, sorted by name for a stable diff.
    pub skills: Vec<SkillMetadata>,
}

/// Location of the committed cross-adapter snapshot.
#[must_use]
pub fn adapters_path() -> PathBuf {
    crate::seeds::dataset_dir().join("adapters.json")
}

/// Harvest every skill dir under `root/relative` for one source.
///
/// Directories the production loader rejects are skipped, not fatal: an
/// adapter may ship a work-in-progress skill, and the benchmark should not
/// block on someone else's repository.
#[must_use]
pub fn harvest_root(root: &Path, source: &AdapterSource, relative: &str) -> Vec<SkillMetadata> {
    let dir = root.join(relative);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();

    let mut skills = Vec::new();
    for skill_dir in dirs {
        if let Some(metadata) = dcc_mcp_skills::parse_skill_md(&skill_dir) {
            skills.push(namespaced(metadata, source, relative, &skill_dir));
        }
    }
    skills
}

/// Rewrite a harvested skill's paths to `<repo>/<root>/<skill>`.
///
/// The production loader fills `skill_path` and `scripts` with real filesystem
/// paths, which would otherwise bake the harvesting machine's clone directory
/// into the committed snapshot. Namespacing by repo name also keeps two
/// adapters that ship a same-named skill distinguishable in a diff.
fn namespaced(
    mut skill: SkillMetadata,
    source: &AdapterSource,
    relative: &str,
    skill_dir: &Path,
) -> SkillMetadata {
    let leaf = skill_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = format!("{}/{relative}/{leaf}", source.name());
    skill.skill_path = format!("{prefix}/SKILL.md");
    skill.scripts = skill
        .scripts
        .iter()
        .map(|script| {
            Path::new(script)
                .file_name()
                .map(|name| format!("{prefix}/{}", name.to_string_lossy()))
                .unwrap_or_else(|| format!("{prefix}/{script}"))
        })
        .collect();
    skill.metadata_files = skill
        .metadata_files
        .iter()
        .map(|file| {
            Path::new(file)
                .file_name()
                .map(|name| format!("{prefix}/{}", name.to_string_lossy()))
                .unwrap_or_else(|| format!("{prefix}/{file}"))
        })
        .collect();
    skill
}

/// Harvest every source under `checkout_root`, where each source lives in a
/// directory named after its repo.
#[must_use]
pub fn harvest_all(checkout_root: &Path, sources: &[AdapterSource]) -> Vec<SkillMetadata> {
    let mut skills = Vec::new();
    for source in sources {
        let root = checkout_root.join(source.name());
        for relative in &source.roots {
            skills.extend(harvest_root(&root, source, relative));
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills.dedup_by(|a, b| a.name == b.name);
    skills
}

/// Build the snapshot for `checkout_root`.
#[must_use]
pub fn build_adapter_set(checkout_root: &Path, sources: &[AdapterSource]) -> AdapterSeedSet {
    AdapterSeedSet {
        schema: crate::synthetic::CORPUS_SCHEMA_VERSION.to_string(),
        sources: sources.to_vec(),
        skills: harvest_all(checkout_root, sources),
    }
}

/// Load the committed snapshot, returning `None` when it is absent or stale.
#[must_use]
pub fn load_committed() -> Option<AdapterSeedSet> {
    let text = std::fs::read_to_string(adapters_path()).ok()?;
    let set: AdapterSeedSet = serde_json::from_str(&text).ok()?;
    (set.schema == crate::synthetic::CORPUS_SCHEMA_VERSION).then_some(set)
}

/// Write `set` to the committed snapshot path.
pub fn write_committed(set: &AdapterSeedSet) -> std::io::Result<()> {
    let path = adapters_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(set).map_err(std::io::Error::other)?;
    text.push('\n');
    std::fs::write(path, text)
}

/// Clone `source` at its pinned commit into `checkout_root/<name>`.
///
/// A full SHA fetch rather than `--branch <tag>`: a branch moves, so the same
/// command would produce a different corpus next week.
pub fn checkout(source: &AdapterSource, checkout_root: &Path) -> std::io::Result<PathBuf> {
    let dest = checkout_root.join(source.name());
    if dest.join(".git").exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::create_dir_all(&dest)?;

    let status = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&dest)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "git init failed for {}",
            source.repo
        )));
    }
    let status = std::process::Command::new("git")
        .args(["remote", "add", "origin", &source.url()])
        .current_dir(&dest)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "git remote add failed for {}",
            source.repo
        )));
    }
    let status = std::process::Command::new("git")
        .args(["fetch", "--depth", "1", "origin", &source.commit])
        .current_dir(&dest)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "git fetch {} failed — is {} a reachable full commit SHA?",
            source.commit, source.repo
        )));
    }
    let status = std::process::Command::new("git")
        .args(["checkout", "--quiet", "FETCH_HEAD"])
        .current_dir(&dest)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "git checkout failed for {}",
            source.repo
        )));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_pinned_to_full_shas() {
        for source in adapter_sources() {
            assert_eq!(
                source.commit.len(),
                40,
                "{} is pinned to {:?}, not a full SHA",
                source.repo,
                source.commit
            );
            assert!(
                source.commit.chars().all(|c| c.is_ascii_hexdigit()),
                "{} has a non-hex commit",
                source.repo
            );
            assert!(!source.roots.is_empty());
        }
    }

    #[test]
    fn adapter_snapshot_is_committed_and_current() {
        let set = load_committed().expect("benchmarks/skills/adapters.json must be committed");
        assert!(
            set.skills.len() >= 80,
            "the cross-adapter snapshot must carry a real catalogue, got {}",
            set.skills.len()
        );
        for skill in &set.skills {
            assert!(!skill.name.is_empty());
            assert!(
                !skill.description.is_empty(),
                "{} has no description",
                skill.name
            );
            assert!(
                Path::new(&skill.skill_path).is_relative(),
                "{}: absolute path left in the snapshot: {}",
                skill.name,
                skill.skill_path
            );
        }
    }

    #[test]
    fn adapter_snapshot_records_its_provenance() {
        let set = load_committed().expect("benchmarks/skills/adapters.json must be committed");
        assert_eq!(set.sources, adapter_sources());
    }
}
