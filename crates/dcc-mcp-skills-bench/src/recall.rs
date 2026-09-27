//! [`RecallContext`] coverage measurement (PIP-3701).
//!
//! `RecallContext` — `app_type` / `domain` / `workflow_stage` /
//! `task_category` — is the only structured discovery signal `SkillMetadata`
//! carries, and all four fields are optional: a missing field is an
//! `unknown`, and the ranker falls back to text similarity.
//!
//! PIP-3215 built the benchmark and PIP-3408 landed the three dimensions, but
//! neither measured how many shipped skills actually populate this struct.
//! Nothing did, because the loader never parsed it: `recall-context` was
//! authored in SKILL.md frontmatter and fell through
//! `apply_dcc_mcp_metadata_overrides` into the "unknown key" arm, so coverage
//! was 0% on skills that declared all four fields.
//!
//! This module is the measurement that was missing. It is deliberately a
//! *count*, not a gate on the ranker: it tells you how much structured signal
//! discovery has to work with, so a change that raises or drops it is visible
//! in the report instead of being inferred from a hit-rate move.

use dcc_mcp_models::SkillMetadata;
use serde::Serialize;

/// The four [`dcc_mcp_models::RecallContext`] fields, in report order.
pub const FIELDS: [&str; 4] = ["app_type", "domain", "workflow_stage", "task_category"];

/// How many of a skill set's [`dcc_mcp_models::RecallContext`] fields are set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct RecallCoverage {
    /// Skills measured.
    pub skills: usize,
    /// Skills whose `recall_context` field is present at all.
    pub with_context: usize,
    /// Skills with an `app_type`.
    pub app_type: usize,
    /// Skills with a `domain`.
    pub domain: usize,
    /// Skills with a `workflow_stage`.
    pub workflow_stage: usize,
    /// Skills with a `task_category`.
    pub task_category: usize,
}

impl RecallCoverage {
    /// Measure `skills`.
    #[must_use]
    pub fn measure(skills: &[SkillMetadata]) -> Self {
        let mut coverage = Self {
            skills: skills.len(),
            ..Self::default()
        };
        for skill in skills {
            let Some(context) = skill.recall_context.as_ref() else {
                continue;
            };
            coverage.with_context += 1;
            coverage.app_type += usize::from(context.app_type.is_some());
            coverage.domain += usize::from(context.domain.is_some());
            coverage.workflow_stage += usize::from(context.workflow_stage.is_some());
            coverage.task_category += usize::from(context.task_category.is_some());
        }
        coverage
    }

    /// Count for [`FIELDS`], by name.
    #[must_use]
    pub fn field(&self, name: &str) -> usize {
        match name {
            "app_type" => self.app_type,
            "domain" => self.domain,
            "workflow_stage" => self.workflow_stage,
            "task_category" => self.task_category,
            _ => 0,
        }
    }

    /// Share of the `4 × skills` field slots that are populated.
    ///
    /// This is the number PIP-3701 gates: a skill missing one of four fields
    /// still contributes three quarters, so "coverage" cannot mean the share
    /// of skills that are fully populated without hiding partial metadata.
    #[must_use]
    pub fn field_coverage(&self) -> f64 {
        if self.skills == 0 {
            return 0.0;
        }
        let filled = self.app_type + self.domain + self.workflow_stage + self.task_category;
        filled as f64 / (FIELDS.len() * self.skills) as f64
    }

    /// Share of skills whose `recall_context` is present at all.
    #[must_use]
    pub fn context_coverage(&self) -> f64 {
        if self.skills == 0 {
            return 0.0;
        }
        self.with_context as f64 / self.skills as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_counts_each_field_separately() {
        let skills = vec![
            SkillMetadata {
                name: "full".to_string(),
                recall_context: Some(dcc_mcp_models::RecallContext {
                    app_type: Some("maya".to_string()),
                    domain: Some("modeling".to_string()),
                    workflow_stage: Some("authoring".to_string()),
                    task_category: Some("mutate".to_string()),
                }),
                ..SkillMetadata::default()
            },
            SkillMetadata {
                name: "partial".to_string(),
                recall_context: Some(dcc_mcp_models::RecallContext {
                    app_type: Some("maya".to_string()),
                    ..dcc_mcp_models::RecallContext::default()
                }),
                ..SkillMetadata::default()
            },
            SkillMetadata {
                name: "none".to_string(),
                ..SkillMetadata::default()
            },
        ];
        let coverage = RecallCoverage::measure(&skills);
        assert_eq!(coverage.skills, 3);
        assert_eq!(coverage.with_context, 2);
        assert_eq!(coverage.app_type, 2);
        assert_eq!(coverage.domain, 1);
        assert_eq!(coverage.workflow_stage, 1);
        assert_eq!(coverage.task_category, 1);
        // 5 filled slots out of 12.
        assert!((coverage.field_coverage() - 5.0 / 12.0).abs() < f64::EPSILON);
        assert!((coverage.context_coverage() - 2.0 / 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_skill_set_has_zero_coverage() {
        let coverage = RecallCoverage::measure(&[]);
        assert_eq!(coverage.field_coverage(), 0.0);
        assert_eq!(coverage.context_coverage(), 0.0);
    }

    /// The gate PIP-3701 sets: the skills **this repository ships** carry all
    /// four fields. Adapter repositories are measured and reported, but their
    /// frontmatter lives in another repository and is out of this PR's reach.
    #[test]
    fn shipped_core_skills_carry_recall_context() {
        let skills = crate::seeds::harvest(&crate::seeds::workspace_root());
        let coverage = RecallCoverage::measure(&skills);
        assert!(
            coverage.field_coverage() >= 0.90,
            "shipped skills carry {:.1}% RecallContext coverage ({} skills, {} with a recall_context block); \
             want >= 90%. Backfill `metadata.dcc-mcp.recall-context` in SKILL.md.",
            coverage.field_coverage() * 100.0,
            coverage.skills,
            coverage.with_context
        );
    }

    /// The loader must actually parse the block, not silently drop it.
    ///
    /// Before PIP-3701 this failed with 0/N: `recall-context` was authored in
    /// `skills/asset-source/SKILL.md` and no code path read it, which is why
    /// the coverage question had no answer.
    #[test]
    fn the_loader_parses_authored_recall_context() {
        let skills = crate::seeds::harvest(&crate::seeds::workspace_root());
        let parsed = skills
            .iter()
            .filter(|skill| skill.recall_context.is_some())
            .count();
        assert!(
            parsed > 0,
            "no shipped skill produced a RecallContext; the loader is dropping metadata.dcc-mcp.recall-context"
        );
    }
}
