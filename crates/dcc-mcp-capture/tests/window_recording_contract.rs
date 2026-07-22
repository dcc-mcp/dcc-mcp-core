use std::time::Duration;

use dcc_mcp_capture::{
    CaptureResult, WindowRecordingConfig, WindowRecordingFrame, WindowRecordingPacer,
    WindowRecordingSchedule, record_window_jpeg_sequence,
};

#[test]
fn constant_rate_schedule_has_exact_bounded_frame_count() {
    for (duration_ms, frames_per_second, expected) in [
        (1_000, 1, 1),
        (1_000, 30, 30),
        (1_500, 24, 36),
        (180_000, 60, 10_800),
    ] {
        let schedule = WindowRecordingSchedule::new(duration_ms, frames_per_second)
            .expect("valid recording schedule");
        assert_eq!(schedule.frame_count(), expected);
        assert_eq!(schedule.deadline(0), Some(Duration::ZERO));
        assert!(schedule.deadline(expected).is_none());
        assert!(schedule.deadline(expected - 1).unwrap() < Duration::from_millis(duration_ms));
    }
}

#[test]
fn schedule_deadlines_are_monotonic_without_accumulated_rounding_drift() {
    let schedule = WindowRecordingSchedule::new(1_000, 60).unwrap();
    let deadlines: Vec<_> = (0..schedule.frame_count())
        .map(|index| schedule.deadline(index).unwrap())
        .collect();
    assert!(deadlines.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(deadlines[30], Duration::from_millis(500));
    assert_eq!(deadlines[59], Duration::from_nanos(983_333_333));
}

#[test]
fn recording_config_requires_an_exact_window_and_host_limits() {
    assert!(WindowRecordingConfig::new(0x1234, 12_000, 30, 92).is_ok());
    for (window_handle, duration_ms, frames_per_second, jpeg_quality) in [
        (0, 12_000, 30, 92),
        (0x1234, 999, 30, 92),
        (0x1234, 180_001, 30, 92),
        (0x1234, 12_000, 0, 92),
        (0x1234, 12_000, 61, 92),
        (0x1234, 12_000, 30, 69),
        (0x1234, 12_000, 30, 101),
    ] {
        assert!(
            WindowRecordingConfig::new(
                window_handle,
                duration_ms,
                frames_per_second,
                jpeg_quality,
            )
            .is_err()
        );
    }
}

#[test]
fn pacer_emits_each_due_index_once_when_source_frames_are_irregular() {
    let schedule = WindowRecordingSchedule::new(1_000, 60).unwrap();
    let mut pacer = WindowRecordingPacer::new(schedule);

    assert_eq!(pacer.take_due(Duration::ZERO), 0..1);
    assert_eq!(pacer.take_due(Duration::from_millis(5)), 1..1);
    assert_eq!(pacer.take_due(Duration::from_millis(20)), 1..2);
    assert_eq!(pacer.take_due(Duration::from_millis(50)), 2..4);
    assert_eq!(pacer.take_due(Duration::from_secs(2)), 4..60);
    assert!(pacer.is_complete());
    assert_eq!(pacer.take_due(Duration::from_secs(3)), 60..60);
}

#[test]
fn recorder_api_streams_frames_to_a_sink_and_accepts_a_cancellation_fence() {
    type Sink = fn(WindowRecordingFrame) -> CaptureResult<()>;
    type CancellationFence = fn() -> bool;
    let _recorder = record_window_jpeg_sequence::<Sink, CancellationFence>;
}
