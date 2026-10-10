use super::*;
use crate::adapters::{SceneNode, SceneObject};
use std::collections::HashMap;

fn connected_adapter() -> MockDccAdapter {
    let mut a = MockDccAdapter::new();
    a.connect().unwrap();
    a
}

#[test]
fn test_get_hierarchy_default() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();
    let tree = hier.get_hierarchy().unwrap();
    // Default: pCube1 + persp as root nodes
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0].object.name, "pCube1");
    assert_eq!(tree[1].object.name, "persp");
}

#[test]
fn test_get_children_root() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();

    // None = root children
    let children = hier.get_children(None).unwrap();
    assert_eq!(children.len(), 2);
}

#[test]
fn test_get_children_named_node_with_children() {
    // Build a hierarchy: root_grp -> [cube, sphere]
    let cube = SceneObject {
        name: "cube".to_string(),
        long_name: "|root_grp|cube".to_string(),
        object_type: "mesh".to_string(),
        parent: Some("|root_grp".to_string()),
        visible: true,
        metadata: HashMap::new(),
    };
    let sphere = SceneObject {
        name: "sphere".to_string(),
        long_name: "|root_grp|sphere".to_string(),
        object_type: "mesh".to_string(),
        parent: Some("|root_grp".to_string()),
        visible: true,
        metadata: HashMap::new(),
    };
    let group = SceneObject {
        name: "root_grp".to_string(),
        long_name: "|root_grp".to_string(),
        object_type: "group".to_string(),
        parent: None,
        visible: true,
        metadata: HashMap::new(),
    };

    let tree = vec![SceneNode {
        object: group,
        children: vec![
            SceneNode {
                object: cube,
                children: vec![],
            },
            SceneNode {
                object: sphere,
                children: vec![],
            },
        ],
    }];

    let config = MockConfig::builder().build();
    let mut adapter = MockDccAdapter::with_config(config);
    adapter.connect().unwrap();
    *adapter.hierarchy.write() = tree;

    let hier = adapter.as_hierarchy().unwrap();
    let children = hier.get_children(Some("root_grp")).unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].name, "cube");
    assert_eq!(children[1].name, "sphere");
}

#[test]
fn test_get_children_leaf_returns_empty() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();
    // pCube1 has no children in default config
    let children = hier.get_children(Some("pCube1")).unwrap();
    assert!(children.is_empty());
}

#[test]
fn test_get_parent_no_parent() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();
    // pCube1 has no parent in default config
    let parent = hier.get_parent("pCube1").unwrap();
    assert!(parent.is_none());
}

#[test]
fn test_get_parent_with_parent() {
    let adapter = connected_adapter();
    // Add an object that has a parent
    adapter.objects.write().push(crate::adapters::SceneObject {
        name: "child".to_string(),
        long_name: "|grp|child".to_string(),
        object_type: "mesh".to_string(),
        parent: Some("|grp".to_string()),
        visible: true,
        metadata: HashMap::new(),
    });
    let hier = adapter.as_hierarchy().unwrap();
    let parent = hier.get_parent("child").unwrap();
    assert_eq!(parent.as_deref(), Some("|grp"));
}

#[test]
fn test_group_objects() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();

    let group = hier
        .group_objects(&["pCube1", "persp"], "myGroup", None)
        .unwrap();
    assert_eq!(group.name, "myGroup");
    assert_eq!(group.object_type, "group");
    assert!(group.parent.is_none());

    // Group should now be in the object list
    let sm = adapter.as_scene_manager().unwrap();
    let objects = sm.list_objects(None).unwrap();
    assert!(objects.iter().any(|o| o.name == "myGroup"));
}

#[test]
fn test_group_objects_with_parent() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();

    let group = hier
        .group_objects(&["pCube1"], "subGroup", Some("|rootGrp"))
        .unwrap();
    assert_eq!(group.parent.as_deref(), Some("|rootGrp"));
}

#[test]
fn test_ungroup() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();

    // First create a group
    hier.group_objects(&["pCube1", "persp"], "tempGroup", None)
        .unwrap();

    // Now ungroup it
    let released = hier.ungroup("tempGroup").unwrap();
    assert_eq!(released, vec!["pCube1", "persp"]);

    // Group should be gone
    let sm = adapter.as_scene_manager().unwrap();
    let objects = sm.list_objects(None).unwrap();
    assert!(!objects.iter().any(|o| o.name == "tempGroup"));
}

#[test]
fn test_ungroup_not_found() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();
    let err = hier.ungroup("doesNotExist").unwrap_err();
    assert_eq!(err.code, DccErrorCode::InvalidInput);
}

#[test]
fn test_reparent() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();

    let updated = hier.reparent("pCube1", Some("|world"), false).unwrap();
    assert_eq!(updated.parent.as_deref(), Some("|world"));

    // Verify via get_parent
    let parent = hier.get_parent("pCube1").unwrap();
    assert_eq!(parent.as_deref(), Some("|world"));
}

#[test]
fn test_reparent_to_root() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();

    // Give pCube1 a parent first
    hier.reparent("pCube1", Some("|grp"), false).unwrap();
    // Then reparent to root
    let updated = hier.reparent("pCube1", None, false).unwrap();
    assert!(updated.parent.is_none());
}

#[test]
fn test_reparent_not_found() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();
    let err = hier.reparent("ghost", Some("|x"), false).unwrap_err();
    assert_eq!(err.code, DccErrorCode::InvalidInput);
}

#[test]
fn test_hierarchy_not_connected() {
    let adapter = MockDccAdapter::new();
    let hier = adapter.as_hierarchy().unwrap();
    assert!(hier.get_hierarchy().is_err());
    assert!(hier.get_children(None).is_err());
    assert!(hier.get_parent("x").is_err());
}

#[test]
fn test_hierarchy_counter_increments() {
    let adapter = connected_adapter();
    let hier = adapter.as_hierarchy().unwrap();
    hier.get_hierarchy().unwrap();
    hier.get_children(None).unwrap();
    hier.get_parent("pCube1").unwrap();
    assert_eq!(adapter.hierarchy_count(), 3);
}
