//! Unit tests for the Python capture bindings.

use super::*;
use std::sync::Once;

static PYTHON_INIT: Once = Once::new();

fn capture(capturer: &PyCapturer, format: &str, jpeg_quality: u8, scale: f32) -> PyCaptureFrame {
    PYTHON_INIT.call_once(Python::initialize);
    Python::attach(|py| capturer.capture(py, format, jpeg_quality, scale, 5000, None, None))
        .unwrap()
}

#[test]
fn test_py_capturer_new_mock() {
    let c = PyCapturer::new_mock(640, 480);
    assert!(c.backend_name().contains("Mock"));
}

#[test]
fn test_py_capturer_capture_png() {
    let c = PyCapturer::new_mock(100, 100);
    let frame = capture(&c, "png", 85, 1.0);
    assert_eq!(frame.format(), "png");
    assert!(frame.data().starts_with(b"\x89PNG"));
    assert_eq!(frame.width(), 100);
    assert_eq!(frame.height(), 100);
    assert!(frame.byte_len() > 0);
}

#[test]
fn test_py_capturer_capture_jpeg() {
    let c = PyCapturer::new_mock(64, 64);
    let frame = capture(&c, "jpeg", 90, 1.0);
    assert_eq!(frame.format(), "jpeg");
    assert_eq!(frame.mime_type(), "image/jpeg");
}

#[test]
fn test_py_capturer_capture_raw() {
    let c = PyCapturer::new_mock(16, 16);
    let frame = capture(&c, "raw_bgra", 85, 1.0);
    assert_eq!(frame.format(), "raw_bgra");
    assert_eq!(frame.byte_len(), 16 * 16 * 4);
}

#[test]
fn test_py_capturer_stats_accumulate() {
    let c = PyCapturer::new_mock(32, 32);
    for _ in 0..3 {
        let _ = capture(&c, "png", 85, 1.0);
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
    let frame = capture(&c, "png", 85, 1.0);
    assert!(frame.__repr__().contains("10x10"));
}

#[test]
fn test_py_capturer_scale_half() {
    let c = PyCapturer::new_mock(200, 100);
    let frame = capture(&c, "raw_bgra", 85, 0.5);
    assert_eq!(frame.width(), 100);
    assert_eq!(frame.height(), 50);
}
