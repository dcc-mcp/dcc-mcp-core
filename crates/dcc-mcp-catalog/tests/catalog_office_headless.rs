//! Bundled Office adapters must include their document-processing backends.

use dcc_mcp_catalog::load_from_str;

#[test]
fn bundled_office_adapters_install_headless_backends() {
    let entries = load_from_str(include_str!("../../../dcc-mcp-catalog.yml")).unwrap();
    for name in ["dcc-mcp-excel", "dcc-mcp-word"] {
        let install = entries
            .iter()
            .find(|entry| entry.name == name)
            .and_then(|entry| entry.install.as_ref())
            .unwrap_or_else(|| panic!("{name} must publish install metadata"));
        assert_eq!(
            install.pip_extras.as_deref(),
            Some(&["headless".to_string()][..]),
            "{name} must install its headless document backend"
        );
    }
}

#[test]
fn bundled_word_pins_release_readme_as_install_instructions() {
    let entries = load_from_str(include_str!("../../../dcc-mcp-catalog.yml")).unwrap();
    let word = entries
        .iter()
        .find(|entry| entry.name == "dcc-mcp-word")
        .unwrap();
    assert_eq!(word.version.as_deref(), Some("0.1.2"));
    let install = word.install.as_ref().unwrap();
    assert_eq!(
        install.instructions_url.as_deref(),
        Some(concat!(
            "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-word/",
            "8e59aa34b34ac23ffc9c419979c5644046561d22/README.md"
        ))
    );
}
