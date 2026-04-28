use super::*;

#[test]
fn valid_frontmatter() {
    let content = "---\nname: test\ndescription: hello\n---\n# Body";
    let fm = extract_frontmatter(content).unwrap();
    assert!(fm.contains("name: test"));
    assert!(fm.contains("description: hello"));
}

#[test]
fn no_frontmatter() {
    assert!(extract_frontmatter("no frontmatter").is_none());
}

#[test]
fn empty_frontmatter() {
    let content = "---\n---\n# Body";
    let fm = extract_frontmatter(content).unwrap();
    assert!(fm.is_empty());
}

#[test]
fn frontmatter_with_lists() {
    let content = "---\nname: test\ntags:\n  - geometry\n  - creation\n---\nBody";
    let fm = extract_frontmatter(content).unwrap();
    assert!(fm.contains("tags:"));
    assert!(fm.contains("- geometry"));
}

#[test]
fn no_closing_delimiter() {
    let content = "---\nname: test\nno closing delimiter";
    assert!(extract_frontmatter(content).is_none());
}
