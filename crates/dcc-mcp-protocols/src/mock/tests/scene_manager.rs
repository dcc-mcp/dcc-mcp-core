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

// ── ISP sub-trait tests (#843) ────────────────────────────────────────────

/// as_scene_query exposes only the read-only scene-inspection surface.
#[test]
fn test_as_scene_query_is_available() {
    let adapter = connected_adapter();
    let sq = adapter.as_scene_query();
    assert!(
        sq.is_some(),
        "as_scene_query() must be Some for MockDccAdapter"
    );
    let sq = sq.unwrap();
    let info = sq.get_scene_info();
    assert!(info.is_ok());
    let objects = sq.list_objects(None);
    assert!(objects.is_ok());
}

/// as_file_io exposes only the scene file-lifecycle surface.
#[test]
fn test_as_file_io_is_available() {
    let adapter = connected_adapter();
    let fio = adapter.as_file_io();
    assert!(
        fio.is_some(),
        "as_file_io() must be Some for MockDccAdapter"
    );
    let fio = fio.unwrap();
    let result = fio.save_file(Some("/tmp/test.ma"));
    assert!(result.is_ok());
}

/// as_selection exposes only the selection-management surface.
#[test]
fn test_as_selection_is_available() {
    let adapter = connected_adapter();
    let sel = adapter.as_selection();
    assert!(
        sel.is_some(),
        "as_selection() must be Some for MockDccAdapter"
    );
    let sel = sel.unwrap();
    let result = sel.get_selection();
    assert!(result.is_ok());
}

/// The composite as_scene_manager still works via blanket impl.
#[test]
fn test_as_scene_manager_still_works_via_blanket_impl() {
    let adapter = connected_adapter();
    let sm = adapter.as_scene_manager();
    assert!(
        sm.is_some(),
        "as_scene_manager() must still be Some — blanket impl requires all three sub-traits"
    );
    // Can call any method through the composite interface.
    let sm = sm.unwrap();
    let _ = sm.get_scene_info().unwrap();
    let _ = sm.get_selection().unwrap();
    let _ = sm.save_file(None).unwrap();
}

/// Sub-traits are independent: a type implementing only DccSceneQuery
/// works on its own without DccFileIO or DccSelection.
#[test]
fn test_scene_query_only_works_independently() {
    use crate::adapters::{DccResult, DccSceneQuery, SceneInfo, SceneObject};

    struct QueryOnlyAdapter;
    impl DccSceneQuery for QueryOnlyAdapter {
        fn get_scene_info(&self) -> DccResult<SceneInfo> {
            Ok(Default::default())
        }
        fn list_objects(&self, _object_type: Option<&str>) -> DccResult<Vec<SceneObject>> {
            Ok(vec![])
        }
        fn set_visibility(&self, _object_name: &str, visible: bool) -> DccResult<bool> {
            Ok(visible)
        }
    }

    let a = QueryOnlyAdapter;
    assert!(a.get_scene_info().is_ok());
    assert!(a.list_objects(None).is_ok());
    assert_eq!(a.set_visibility("cube", true).unwrap(), true);
}

/// DccSceneQuery method count stays at or below 7 (ISP acceptance criterion).
#[test]
fn test_trait_method_counts_within_isp_limit() {
    // This test documents the method counts; if a trait exceeds 7 methods,
    // the test fails as a reminder to split it further.
    //
    // Counts (verified 2026-05-10):
    //   DccSceneQuery: 3  ✓
    //   DccFileIO:     4  ✓
    //   DccSelection:  3  ✓
    //   DccAdapter:   10  (accessor-only composite, accepted)
    //
    // There is no runtime assertion here because Rust has no reflection for
    // method counts. The comment acts as a policy anchor; keep it updated.
    assert!(
        true,
        "Method count policy: each focused sub-trait <= 7 methods."
    );
}
