use super::*;

#[test]
fn test_get_scene_info() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    let info = DccSceneInfo::get_scene_info(&adapter).unwrap();
    assert_eq!(info.name, "untitled");
    assert!(!info.modified);
    assert_eq!(adapter.scene_query_count(), 1);
}

#[test]
fn test_set_scene() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    adapter.set_scene(SceneInfo {
        file_path: "/projects/shot_010.ma".to_string(),
        name: "shot_010".to_string(),
        modified: true,
        format: ".ma".to_string(),
        statistics: SceneStatistics {
            object_count: 100,
            vertex_count: 50000,
            ..Default::default()
        },
        ..Default::default()
    });

    let info = DccSceneInfo::get_scene_info(&adapter).unwrap();
    assert_eq!(info.name, "shot_010");
    assert!(info.modified);
    assert_eq!(info.statistics.object_count, 100);
}

#[test]
fn test_set_modified() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    adapter.set_modified(true);
    assert!(DccSceneInfo::get_scene_info(&adapter).unwrap().modified);

    adapter.set_modified(false);
    assert!(!DccSceneInfo::get_scene_info(&adapter).unwrap().modified);
}

#[test]
fn test_list_objects() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    adapter.set_statistics(SceneStatistics {
        object_count: 3,
        light_count: 2,
        camera_count: 1,
        ..Default::default()
    });

    let objects = DccSceneInfo::list_objects(&adapter).unwrap();
    assert_eq!(objects.len(), 6); // 3 mesh + 2 light + 1 camera
    assert_eq!(objects[0].1, "mesh");
    assert_eq!(objects[3].1, "light");
    assert_eq!(objects[5].1, "camera");
}

#[test]
fn test_get_selection() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    let selection = DccSceneInfo::get_selection(&adapter).unwrap();
    assert!(selection.is_empty());
}

#[test]
fn test_scene_query_not_connected() {
    let adapter = MockDccAdapter::new();
    assert!(DccSceneInfo::get_scene_info(&adapter).is_err());
    assert!(DccSceneInfo::list_objects(&adapter).is_err());
    assert!(DccSceneInfo::get_selection(&adapter).is_err());
}
