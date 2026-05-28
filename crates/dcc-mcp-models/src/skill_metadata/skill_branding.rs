//! Branding / link / example-prompt metadata authored in SKILL.md.
//!
//! These structures are optional and parsed from
//! `metadata.dcc-mcp.{branding,links,example-prompts}` so existing
//! skills stay valid without touching their frontmatter. They power
//! the Admin UI marketplace surface — when a skill ships a
//! `branding.accent_color` or `links.docs` we render it on the card;
//! otherwise the card falls back to a name-derived accent + DCC icon.

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pyclass;
use serde::{Deserialize, Serialize};

/// Author-supplied visual identity for a skill's marketplace card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillBranding", get_all, from_py_object)
)]
pub struct SkillBranding {
    /// CSS-friendly primary accent (e.g. `"#ff7a45"`, `"hsl(28 90% 56%)"`).
    /// Falls back to a hash-derived hue when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,

    /// Secondary accent for gradient backgrounds. Optional.
    #[serde(
        default,
        rename = "secondary-color",
        alias = "secondary_color",
        skip_serializing_if = "Option::is_none"
    )]
    pub secondary_color: Option<String>,

    /// Short brand glyph — typically an emoji or unicode mark. Cards
    /// prefer this over a DCC icon when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,

    /// HTTPS URL to a square SVG or PNG logo. When provided the card
    /// shows the logo in place of both `emoji` and the DCC icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,

    /// One-line tagline shown under the skill name. Plain text, no markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
}

/// Author-supplied external references rendered as a chip row on the card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillLinks", get_all, from_py_object)
)]
pub struct SkillLinks {
    /// Documentation entry point (most prominent link).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,

    /// Public source repository URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,

    /// Marketing / project homepage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// Issue tracker URL — surfaces a quick-feedback chip on the card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issues: Option<String>,

    /// Public chat / support channel — Discord, Slack invite, etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
}

impl SkillBranding {
    /// Cheap "is any field set" check used by serialisers to skip
    /// emitting an empty `branding` block.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.accent_color.is_none()
            && self.secondary_color.is_none()
            && self.emoji.is_none()
            && self.logo_url.is_none()
            && self.tagline.is_none()
    }
}

impl SkillLinks {
    /// Cheap "is any link set" check used by serialisers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.docs.is_none()
            && self.repo.is_none()
            && self.homepage.is_none()
            && self.issues.is_none()
            && self.chat.is_none()
    }
}
