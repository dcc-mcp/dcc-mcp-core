use super::*;
use crate::adapters::BoundingBox;

fn connected_adapter() -> MockDccAdapter {
    let mut a = MockDccAdapter::new();
    a.connect().unwrap();
    a
}

#[test]
fn test_get_transform_default_object() {
    let adapter = connected_adapter();
    let tf = adapter.as_transform().unwrap();

    let t = tf.get_transform("pCube1").unwrap();
    // Default config registers identity transform for pCube1
    assert_eq!(t.translate, [0.0, 0.0, 0.0]);
    assert_eq!(t.rotate, [0.0, 0.0, 0.0]);
    assert_eq!(t.scale, [1.0, 1.0, 1.0]);
}

#[test]
fn test_get_transform_not_found() {
    let adapter = connected_adapter();
    let tf = adapter.as_transform().unwrap();
    let err = tf.get_transform("nonexistent").unwrap_err();
    assert_eq!(err.code, DccErrorCode::InvalidInput);
}

#[test]
fn test_set_transform_translate_only() {
    let adapter = connected_adapter();
    let tf = adapter.as_transform().unwrap();

    let result = tf
        .set_transform("pCube1", Some([10.0, 20.0, 30.0]), None, None)
        .unwrap();
    assert_eq!(result.translate, [10.0, 20.0, 30.0]);
    assert_eq!(result.rotate, [0.0, 0.0, 0.0]); // unchanged
    assert_eq!(result.scale, [1.0, 1.0, 1.0]); // unchanged
}

#[test]
fn test_set_transform_all_components() {
    let adapter = connected_adapter();
    let tf = adapter.as_transform().unwrap();

    let result = tf
        .set_transform(
            "pCube1",
            Some([5.0, 0.0, -5.0]),
            Some([0.0, 45.0, 0.0]),
            Some([2.0, 2.0, 2.0]),
        )
        .unwrap();
    assert_eq!(result.translate, [5.0, 0.0, -5.0]);
    assert_eq!(result.rotate, [0.0, 45.0, 0.0]);
    assert_eq!(result.scale, [2.0, 2.0, 2.0]);

    // Confirm persisted
    let fetched = tf.get_transform("pCube1").unwrap();
    assert_eq!(fetched.translate, [5.0, 0.0, -5.0]);
}

#[test]
fn test_set_transform_creates_new_object() {
    let adapter = connected_adapter();
    let tf = adapter.as_transform().unwrap();

    // "newObj" doesn't exist yet — set_transform should create an identity entry
    let result = tf
        .set_transform("newObj", Some([1.0, 2.0, 3.0]), None, None)
        .unwrap();
    assert_eq!(result.translate, [1.0, 2.0, 3.0]);
    assert_eq!(result.scale, [1.0, 1.0, 1.0]); // identity default
}

#[test]
fn test_get_bounding_box() {
    let adapter = connected_adapter();
    let tf = adapter.as_transform().unwrap();

    let bb = tf.get_bounding_box("pCube1").unwrap();
    assert_eq!(bb.min, [-1.0, 0.0, -1.0]);
    assert_eq!(bb.max, [1.0, 2.0, 1.0]);
    assert_eq!(bb.center(), [0.0, 1.0, 0.0]);
    assert_eq!(bb.size(), [2.0, 2.0, 2.0]);
}

#[test]
fn test_get_bounding_box_not_found() {
    let adapter = connected_adapter();
    let tf = adapter.as_transform().unwrap();
    let err = tf.get_bounding_box("missing").unwrap_err();
    assert_eq!(err.code, DccErrorCode::InvalidInput);
}

#[test]
fn test_register_bounding_box_helper() {
    let adapter = connected_adapter();
    adapter.register_bounding_box(
        "light_01",
        BoundingBox {
            min: [0.0, 0.0, 0.0],
            max: [0.1, 0.1, 0.1],
        },
    );
    let tf = adapter.as_transform().unwrap();
    let bb = tf.get_bounding_box("light_01").unwrap();
    assert_eq!(bb.size(), [0.1, 0.1, 0.1]);
}

#[test]
fn test_rename_object() {
    let adapter = connected_adapter();
    let tf = adapter.as_transform().unwrap();

    let new_name = tf.rename_object("pCube1", "myCube").unwrap();
    assert_eq!(new_name, "myCube");

    // Original name is gone from object list
    let sm = adapter.as_scene_manager().unwrap();
    let meshes = sm.list_objects(Some("mesh")).unwrap();
    assert_eq!(meshes[0].name, "myCube");
}

#[test]
fn test_rename_object_not_found() {
    let adapter = connected_adapter();
    let tf = adapter.as_transform().unwrap();
    let err = tf.rename_object("ghost", "new").unwrap_err();
    assert_eq!(err.code, DccErrorCode::InvalidInput);
}

#[test]
fn test_transform_not_connected() {
    let adapter = MockDccAdapter::new();
    let tf = adapter.as_transform().unwrap();
    assert!(tf.get_transform("pCube1").is_err());
    assert!(tf.set_transform("pCube1", None, None, None).is_err());
}
