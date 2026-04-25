//! Unit tests for the Python capture bindings.

use super::*;

#[test]
fn test_py_capturer_new_mock() {
    let c = PyCapturer::new_mock(640, 480);
    assert!(c.backend_name().contains("Mock"));
}

#[test]
fn test_py_capturer_capture_png() {
    let c = PyCapturer::new_mock(100, 100);
    let frame = c.capture("png", 85, 1.0, 5000, None, None).unwrap();
    assert_eq!(frame.format(), "png");
    assert!(frame.data().starts_with(b"\x89PNG"));
    assert_eq!(frame.width(), 100);
    assert_eq!(frame.height(), 100);
    assert!(frame.byte_len() > 0);
}

#[test]
fn test_py_capturer_capture_jpeg() {
    let c = PyCapturer::new_mock(64, 64);
    let frame = c.capture("jpeg", 90, 1.0, 5000, None, None).unwrap();
    assert_eq!(frame.format(), "jpeg");
    assert_eq!(frame.mime_type(), "image/jpeg");
}

#[test]
fn test_py_capturer_capture_raw() {
    let c = PyCapturer::new_mock(16, 16);
    let frame = c.capture("raw_bgra", 85, 1.0, 5000, None, None).unwrap();
    assert_eq!(frame.format(), "raw_bgra");
    assert_eq!(frame.byte_len(), 16 * 16 * 4);
}

#[test]
fn test_py_capturer_stats_accumulate() {
    let c = PyCapturer::new_mock(32, 32);
    for _ in 0..3 {
        let _ = c.capture("png", 85, 1.0, 5000, None, None).unwrap();
    }
    let (count, bytes, errs) = c.stats();
    assert_eq!(count, 3);
    assert!(bytes > 0);
    assert_eq!(errs, 0);
}

#[test]
fn test_py_capturer_new_auto_backend_name_nonempty() {
    let c = PyCapturer::new_auto();
    assert!(!c.backend_name().is_empty());
}

#[test]
fn test_py_capturer_repr() {
    let c = PyCapturer::new_mock(1, 1);
    assert!(c.__repr__().contains("Capturer"));
}

#[test]
fn test_py_capture_frame_repr() {
    let c = PyCapturer::new_mock(10, 10);
    let frame = c.capture("png", 85, 1.0, 5000, None, None).unwrap();
    assert!(frame.__repr__().contains("10x10"));
}

#[test]
fn test_py_capturer_scale_half() {
    let c = PyCapturer::new_mock(200, 100);
    let frame = c.capture("raw_bgra", 85, 0.5, 5000, None, None).unwrap();
    assert_eq!(frame.width(), 100);
    assert_eq!(frame.height(), 50);
}
