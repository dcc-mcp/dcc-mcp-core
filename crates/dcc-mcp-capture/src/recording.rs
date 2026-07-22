//! Exact-window recording domain types shared by the Windows capture backend and host.

use std::time::Duration;

use crate::error::{CaptureError, CaptureResult};

const MIN_DURATION_MS: u64 = 1_000;
const MAX_DURATION_MS: u64 = 180_000;
const MIN_FRAMES_PER_SECOND: u32 = 1;
const MAX_FRAMES_PER_SECOND: u32 = 60;
const MIN_JPEG_QUALITY: u8 = 70;
const MAX_JPEG_QUALITY: u8 = 100;

/// Validated exact-window JPEG-sequence recording request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRecordingConfig {
    window_handle: u64,
    duration_ms: u64,
    frames_per_second: u32,
    jpeg_quality: u8,
}

impl WindowRecordingConfig {
    /// Validate and create one bounded exact-window recording request.
    pub fn new(
        window_handle: u64,
        duration_ms: u64,
        frames_per_second: u32,
        jpeg_quality: u8,
    ) -> CaptureResult<Self> {
        if window_handle == 0 {
            return Err(CaptureError::InvalidConfig(
                "exact-window recording requires a non-zero window handle".to_owned(),
            ));
        }
        if !(MIN_DURATION_MS..=MAX_DURATION_MS).contains(&duration_ms)
            || !(MIN_FRAMES_PER_SECOND..=MAX_FRAMES_PER_SECOND).contains(&frames_per_second)
            || !(MIN_JPEG_QUALITY..=MAX_JPEG_QUALITY).contains(&jpeg_quality)
        {
            return Err(CaptureError::InvalidConfig(
                "duration_ms must be 1000..=180000, frames_per_second must be 1..=60, and jpeg_quality must be 70..=100"
                    .to_owned(),
            ));
        }
        Ok(Self {
            window_handle,
            duration_ms,
            frames_per_second,
            jpeg_quality,
        })
    }

    /// Exact HWND represented as an unsigned integer.
    pub fn window_handle(self) -> u64 {
        self.window_handle
    }

    /// Requested clip duration in milliseconds.
    pub fn duration_ms(self) -> u64 {
        self.duration_ms
    }

    /// Requested constant output rate.
    pub fn frames_per_second(self) -> u32 {
        self.frames_per_second
    }

    /// JPEG encoder quality.
    pub fn jpeg_quality(self) -> u8 {
        self.jpeg_quality
    }

    /// Deterministic constant-rate output schedule.
    pub fn schedule(self) -> WindowRecordingSchedule {
        WindowRecordingSchedule::new(self.duration_ms, self.frames_per_second)
            .expect("validated recording config must produce a valid schedule")
    }
}

/// Rational constant-rate schedule calculated from absolute time offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRecordingSchedule {
    frames_per_second: u32,
    frame_count: u32,
}

impl WindowRecordingSchedule {
    /// Build a bounded schedule without accumulating per-frame rounding error.
    pub fn new(duration_ms: u64, frames_per_second: u32) -> CaptureResult<Self> {
        if !(MIN_DURATION_MS..=MAX_DURATION_MS).contains(&duration_ms)
            || !(MIN_FRAMES_PER_SECOND..=MAX_FRAMES_PER_SECOND).contains(&frames_per_second)
        {
            return Err(CaptureError::InvalidConfig(
                "recording schedule exceeds the host duration or frame-rate boundary".to_owned(),
            ));
        }
        let numerator = duration_ms
            .checked_mul(u64::from(frames_per_second))
            .ok_or_else(|| CaptureError::InvalidConfig("frame count overflowed".to_owned()))?;
        let frame_count = numerator.div_ceil(1_000).try_into().map_err(|_| {
            CaptureError::InvalidConfig("frame count does not fit in u32".to_owned())
        })?;
        Ok(Self {
            frames_per_second,
            frame_count,
        })
    }

    /// Number of constant-rate output frames.
    pub fn frame_count(self) -> u32 {
        self.frame_count
    }

    /// Absolute deadline from clip start for an output frame index.
    pub fn deadline(self, index: u32) -> Option<Duration> {
        if index >= self.frame_count {
            return None;
        }
        let nanoseconds = u64::from(index)
            .checked_mul(1_000_000_000)?
            .checked_div(u64::from(self.frames_per_second))?;
        Some(Duration::from_nanos(nanoseconds))
    }
}

/// Stateful cursor that maps irregular source-frame times onto one CFR schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRecordingPacer {
    schedule: WindowRecordingSchedule,
    next_index: u32,
}

impl WindowRecordingPacer {
    /// Start at output frame zero.
    pub fn new(schedule: WindowRecordingSchedule) -> Self {
        Self {
            schedule,
            next_index: 0,
        }
    }

    /// Consume every output index whose absolute deadline has elapsed.
    pub fn take_due(&mut self, elapsed: Duration) -> std::ops::Range<u32> {
        let start = self.next_index;
        while self
            .schedule
            .deadline(self.next_index)
            .is_some_and(|deadline| deadline <= elapsed)
        {
            self.next_index += 1;
        }
        start..self.next_index
    }

    /// Whether all scheduled output frames were consumed.
    pub fn is_complete(self) -> bool {
        self.next_index == self.schedule.frame_count()
    }

    /// Deadline for the next output frame, if any.
    pub fn next_deadline(self) -> Option<Duration> {
        self.schedule.deadline(self.next_index)
    }
}

/// One encoded frame yielded incrementally by the recorder.
#[derive(Debug)]
pub struct WindowRecordingFrame {
    /// Constant-rate output index.
    pub index: u32,
    /// Capture timestamp in Unix epoch milliseconds.
    pub timestamp_ms: u64,
    /// Encoded JPEG bytes.
    pub data: Vec<u8>,
    /// Physical pixel width.
    pub width: u32,
    /// Physical pixel height.
    pub height: u32,
}

/// Completed exact-window recording summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRecordingSummary {
    /// Number of frames yielded to the sink.
    pub frame_count: u32,
    /// First frame timestamp in Unix epoch milliseconds.
    pub started_at_ms: u64,
    /// Completion timestamp in Unix epoch milliseconds.
    pub ended_at_ms: u64,
    /// Physical pixel width shared by every frame.
    pub width: u32,
    /// Physical pixel height shared by every frame.
    pub height: u32,
}
