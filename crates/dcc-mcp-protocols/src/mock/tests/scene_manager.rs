use super::*;
use crate::adapters::SceneObject;
use std::collections::HashMap;

fn connected_adapter() -> MockDccAdapter {
    let mut a = MockDccAdapter::new();
    a.connect().unwrap();
    a
}

#[test]
fn test_list_objects_all() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();
    let objects = sm.list_objects(None).unwrap();
    // Default config has pCube1 (mesh) + persp (camera)
    assert_eq!(objects.len(), 2);
}

#[test]
fn test_list_objects_filtered_by_type() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();

    let meshes = sm.list_objects(Some("mesh")).unwrap();
    assert_eq!(meshes.len(), 1);
    assert_eq!(meshes[0].name, "pCube1");

    let cameras = sm.list_objects(Some("camera")).unwrap();
    assert_eq!(cameras.len(), 1);
    assert_eq!(cameras[0].name, "persp");

    let lights = sm.list_objects(Some("light")).unwrap();
    assert!(lights.is_empty());
}

#[test]
fn test_list_objects_not_connected() {
    let adapter = MockDccAdapter::new();
    let sm = adapter.as_scene_manager().unwrap();
    assert!(sm.list_objects(None).is_err());
}

#[test]
fn test_get_set_selection() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();

    // Initially empty
    assert!(sm.get_selection().unwrap().is_empty());

    // Set selection
    let selected = sm.set_selection(&["pCube1", "persp"]).unwrap();
    assert_eq!(selected.len(), 2);
    assert!(selected.contains(&"pCube1".to_string()));

    // Read back
    let current = sm.get_selection().unwrap();
    assert_eq!(current, vec!["pCube1", "persp"]);
}

#[test]
fn test_select_by_type() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();

    let meshes = sm.select_by_type("mesh").unwrap();
    assert_eq!(meshes, vec!["pCube1"]);

    // Selection should be updated
    assert_eq!(sm.get_selection().unwrap(), vec!["pCube1"]);
}

#[test]
fn test_set_visibility() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();

    // Hide the cube
    let result = sm.set_visibility("pCube1", false).unwrap();
    assert!(!result);

    // Verify via list_objects
    let objects = sm.list_objects(Some("mesh")).unwrap();
    assert!(!objects[0].visible);

    // Show it again
    sm.set_visibility("pCube1", true).unwrap();
    let objects = sm.list_objects(Some("mesh")).unwrap();
    assert!(objects[0].visible);
}

#[test]
fn test_set_visibility_not_found() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();
    let err = sm.set_visibility("nonexistent", true).unwrap_err();
    assert_eq!(err.code, DccErrorCode::InvalidInput);
}

#[test]
fn test_new_scene() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();

    let info = sm.new_scene(false).unwrap();
    assert_eq!(info.name, "untitled");
    assert!(!info.modified);

    // Objects should be cleared
    let objects = sm.list_objects(None).unwrap();
    assert!(objects.is_empty());
}

#[test]
fn test_open_file() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();

    let info = sm.open_file("/projects/shot_010.ma", false).unwrap();
    assert_eq!(info.file_path, "/projects/shot_010.ma");
    assert_eq!(info.name, "shot_010.ma");
    assert!(!info.modified);
}

#[test]
fn test_save_file_inplace() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();

    // Open a file first to set a path
    sm.open_file("/projects/shot.ma", false).unwrap();
    let saved = sm.save_file(None).unwrap();
    assert_eq!(saved, "/projects/shot.ma");
}

#[test]
fn test_save_file_new_path() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();
    let saved = sm.save_file(Some("/export/final.ma")).unwrap();
    assert_eq!(saved, "/export/final.ma");
}

#[test]
fn test_export_file() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager().unwrap();
    let path = sm.export_file("/export/model.fbx", "fbx", false).unwrap();
    assert_eq!(path, "/export/model.fbx");
}

#[test]
fn test_set_objects_via_helper() {
    let adapter = connected_adapter();
    adapter.set_objects(vec![
        SceneObject {
            name: "cube".to_string(),
            long_name: "|cube".to_string(),
            object_type: "mesh".to_string(),
            parent: None,
            visible: true,
            metadata: HashMap::new(),
        },
        SceneObject {
            name: "sphere".to_string(),
            long_name: "|sphere".to_string(),
            object_type: "mesh".to_string(),
            parent: None,
            visible: true,
            metadata: HashMap::new(),
        },
    ]);
    let sm = adapter.as_scene_manager().unwrap();
    let objects = sm.list_objects(None).unwrap();
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0].name, "cube");
    assert_eq!(objects[1].name, "sphere");
}
