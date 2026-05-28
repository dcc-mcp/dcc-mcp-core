//! Tests for `metadata.dcc-mcp.{branding,links,example-prompts}` parsing.
//!
//! These three keys feed the Admin UI marketplace card surface (Track D
//! / #1407). The loader must populate `SkillMetadata::branding`,
//! `links`, and `example_prompts` only when the canonical nested form
//! is used; empty author input must stay `None` / empty so the UI can
//! cleanly distinguish "unset" from "explicitly blank".

use super::fixtures::write_skill;
use super::*;

#[test]
fn branding_block_populates_typed_field() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("nested_branding");
    write_skill(
        &dir,
        r##"---
name: nested-branding
description: A skill with custom marketplace branding.
metadata:
  dcc-mcp:
    dcc: maya
    branding:
      accent_color: "#ff7a45"
      emoji: "🐉"
      tagline: "High-impact bevel and retopo flow"
    links:
      docs: https://example.com/docs
      repo: https://github.com/example/skill
    example-prompts:
      - "Bevel the selected edges with mitred corners"
      - "Retopologise the hi-poly mesh down to a quad cage"
---
"##,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    let branding = meta.branding.expect("branding populated");
    assert_eq!(branding.accent_color.as_deref(), Some("#ff7a45"));
    assert_eq!(branding.emoji.as_deref(), Some("🐉"));
    assert_eq!(
        branding.tagline.as_deref(),
        Some("High-impact bevel and retopo flow")
    );
    let links = meta.links.expect("links populated");
    assert_eq!(links.docs.as_deref(), Some("https://example.com/docs"));
    assert_eq!(
        links.repo.as_deref(),
        Some("https://github.com/example/skill")
    );
    assert_eq!(meta.example_prompts.len(), 2);
    assert!(meta.example_prompts[0].contains("Bevel"));
}

#[test]
fn empty_branding_block_keeps_field_unset() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("empty_branding");
    write_skill(
        &dir,
        r#"---
name: empty-branding
description: A skill with no marketplace fields authored.
metadata:
  dcc-mcp:
    dcc: blender
    branding: {}
    links: {}
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(
        meta.branding.is_none(),
        "branding with no fields must stay None so the UI uses the hash-derived fallback"
    );
    assert!(
        meta.links.is_none(),
        "links with no fields must stay None so the chip row is hidden entirely"
    );
    assert!(meta.example_prompts.is_empty());
}

#[test]
fn example_prompts_accept_csv_string_form() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("example_csv");
    write_skill(
        &dir,
        r#"---
name: example-csv
description: Author wrote example-prompts as a comma-separated string.
metadata:
  dcc-mcp:
    dcc: houdini
    example-prompts: "Submit Karma render, Inspect AOVs"
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(meta.example_prompts.len(), 2);
    assert_eq!(meta.example_prompts[0], "Submit Karma render");
    assert_eq!(meta.example_prompts[1], "Inspect AOVs");
}
