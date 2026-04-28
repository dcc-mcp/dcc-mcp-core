//! Shared test helpers for registry tests.

use super::*;

pub fn make_action(name: &str, dcc: &str) -> ActionMeta {
    ActionMeta {
        name: name.into(),
        description: format!("{name} description"),
        category: "geometry".into(),
        tags: vec!["test".into()],
        dcc: dcc.into(),
        version: "1.0.0".into(),
        ..Default::default()
    }
}

pub fn make_rich_action(name: &str, dcc: &str, category: &str, tags: Vec<&str>) -> ActionMeta {
    ActionMeta {
        name: name.into(),
        description: format!("{name} desc"),
        category: category.into(),
        tags: tags.into_iter().map(String::from).collect(),
        dcc: dcc.into(),
        version: "1.0.0".into(),
        ..Default::default()
    }
}

pub fn populate_search_registry() -> ActionRegistry {
    let reg = ActionRegistry::new();
    reg.register_action(make_rich_action(
        "create_sphere",
        "maya",
        "geometry",
        vec!["create", "mesh"],
    ));
    reg.register_action(make_rich_action(
        "delete_mesh",
        "maya",
        "geometry",
        vec!["delete", "mesh"],
    ));
    reg.register_action(make_rich_action(
        "render_scene",
        "maya",
        "rendering",
        vec!["render", "output"],
    ));
    reg.register_action(make_rich_action(
        "create_cube",
        "blender",
        "geometry",
        vec!["create", "mesh"],
    ));
    reg.register_action(make_rich_action(
        "bake_texture",
        "blender",
        "rendering",
        vec!["bake", "texture", "render"],
    ));
    reg
}
