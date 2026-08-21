use dcc_mcp_catalog::{CatalogSearchHit, load_from_str, materialise_page};

#[allow(deprecated)]
use dcc_mcp_catalog::SearchHit;

fn assert_canonical_hit(_: CatalogSearchHit) {}

#[test]
#[allow(deprecated)]
fn legacy_search_hit_is_the_canonical_catalog_type() {
    let legacy = SearchHit { index: 0, score: 7 };

    assert_canonical_hit(legacy);
}

#[test]
fn materialise_page_accepts_catalog_search_hits() {
    let entries = load_from_str(
        r#"{
            "entries": [
                {"name": "dcc-mcp-maya", "description": "Maya adapter", "dcc": ["maya"]},
                {"name": "dcc-mcp-photoshop", "description": "Photoshop adapter", "dcc": ["photoshop"]}
            ]
        }"#,
    )
    .expect("catalog fixture should parse");
    let hits = [
        CatalogSearchHit { index: 1, score: 9 },
        CatalogSearchHit { index: 0, score: 4 },
    ];

    let page = materialise_page(&entries, &hits);

    assert_eq!(page[0].name, "dcc-mcp-photoshop");
    assert_eq!(page[1].name, "dcc-mcp-maya");
}
