use std::fs;
use std::path::{Path, PathBuf};

use dcc_mcp_artefact::{atomic_write_bytes, hash_bytes_sha256};
use dcc_mcp_ui_control::host_protocol::{UiControlClipArtifact, UiControlTarget};
use serde::Serialize;
use thiserror::Error;

const MANIFEST_VERSION: u32 = 1;
const FRAME_PATTERN: &str = "frame-%06d.jpg";

#[derive(Debug, Error)]
pub(super) enum RecordingArtifactError {
    #[error("invalid recording artifact: {0}")]
    Invalid(String),
    #[error("recording artifact I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("recording manifest serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Debug, Serialize)]
struct RecordingEncoding {
    format: &'static str,
    frames_per_second: u32,
    jpeg_quality: u8,
}

#[derive(Debug, Serialize)]
struct RecordingDimensions {
    width: u32,
    height: u32,
}

#[derive(Debug, Serialize)]
struct RecordingFrameEntry {
    index: u32,
    path: String,
    timestamp_ms: u64,
    byte_length: usize,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct RecordingManifest<'a> {
    manifest_version: u32,
    recording_id: &'a str,
    target: &'a UiControlTarget,
    encoding: RecordingEncoding,
    dimensions: RecordingDimensions,
    started_at_ms: u64,
    ended_at_ms: u64,
    frames: &'a [RecordingFrameEntry],
}

pub(super) struct RecordingArtifactWriter {
    recording_id: String,
    directory: PathBuf,
    target: UiControlTarget,
    frames_per_second: u32,
    jpeg_quality: u8,
    dimensions: Option<(u32, u32)>,
    frames: Vec<RecordingFrameEntry>,
    committed: bool,
}

impl RecordingArtifactWriter {
    pub(super) fn create_in(
        root: &Path,
        recording_id: &str,
        target: UiControlTarget,
        frames_per_second: u32,
        jpeg_quality: u8,
    ) -> Result<Self, RecordingArtifactError> {
        if !valid_recording_id(recording_id) {
            return Err(RecordingArtifactError::Invalid(
                "recording_id must contain only ASCII letters, digits, '-' or '_'".to_owned(),
            ));
        }
        fs::create_dir_all(root)?;
        let canonical_root = fs::canonicalize(root)?;
        let directory = canonical_root.join(recording_id);
        fs::create_dir(&directory)?;
        let canonical_directory = fs::canonicalize(&directory)?;
        if canonical_directory.parent() != Some(canonical_root.as_path()) {
            let _ = fs::remove_dir_all(&canonical_directory);
            return Err(RecordingArtifactError::Invalid(
                "recording directory escaped the host-owned root".to_owned(),
            ));
        }
        Ok(Self {
            recording_id: recording_id.to_owned(),
            directory: canonical_directory,
            target,
            frames_per_second,
            jpeg_quality,
            dimensions: None,
            frames: Vec::new(),
            committed: false,
        })
    }

    #[cfg(test)]
    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(super) fn write_frame(
        &mut self,
        index: u32,
        timestamp_ms: u64,
        width: u32,
        height: u32,
        jpeg_bytes: &[u8],
    ) -> Result<(), RecordingArtifactError> {
        if self.committed {
            return Err(RecordingArtifactError::Invalid(
                "recording is already committed".to_owned(),
            ));
        }
        if index as usize != self.frames.len() {
            return Err(RecordingArtifactError::Invalid(format!(
                "frame index {index} is not the next sequential index {}",
                self.frames.len()
            )));
        }
        if width == 0 || height == 0 || jpeg_bytes.is_empty() {
            return Err(RecordingArtifactError::Invalid(
                "recording frames require non-zero dimensions and bytes".to_owned(),
            ));
        }
        if let Some((expected_width, expected_height)) = self.dimensions {
            if (width, height) != (expected_width, expected_height) {
                return Err(RecordingArtifactError::Invalid(format!(
                    "frame dimensions changed from {expected_width}x{expected_height} to {width}x{height}"
                )));
            }
        } else {
            self.dimensions = Some((width, height));
        }
        if self
            .frames
            .last()
            .is_some_and(|previous| timestamp_ms < previous.timestamp_ms)
        {
            return Err(RecordingArtifactError::Invalid(
                "frame timestamps must be monotonic".to_owned(),
            ));
        }

        let relative_path = format!("frame-{index:06}.jpg");
        atomic_write_bytes(&self.directory.join(&relative_path), jpeg_bytes, false)?;
        self.frames.push(RecordingFrameEntry {
            index,
            path: relative_path,
            timestamp_ms,
            byte_length: jpeg_bytes.len(),
            sha256: hash_bytes_sha256(jpeg_bytes),
        });
        Ok(())
    }

    pub(super) fn finish(
        &mut self,
        ended_at_ms: u64,
    ) -> Result<UiControlClipArtifact, RecordingArtifactError> {
        if self.committed {
            return Err(RecordingArtifactError::Invalid(
                "recording is already committed".to_owned(),
            ));
        }
        let (width, height) = self.dimensions.ok_or_else(|| {
            RecordingArtifactError::Invalid("recording contains no frames".to_owned())
        })?;
        let started_at_ms = self.frames[0].timestamp_ms;
        if ended_at_ms < self.frames.last().unwrap().timestamp_ms {
            return Err(RecordingArtifactError::Invalid(
                "recording end precedes the final frame".to_owned(),
            ));
        }
        let manifest = RecordingManifest {
            manifest_version: MANIFEST_VERSION,
            recording_id: &self.recording_id,
            target: &self.target,
            encoding: RecordingEncoding {
                format: "jpeg_sequence",
                frames_per_second: self.frames_per_second,
                jpeg_quality: self.jpeg_quality,
            },
            dimensions: RecordingDimensions { width, height },
            started_at_ms,
            ended_at_ms,
            frames: &self.frames,
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
        let manifest_path = self.directory.join("manifest.json");
        atomic_write_bytes(&manifest_path, &manifest_bytes, false)?;
        let artifact = UiControlClipArtifact {
            recording_id: self.recording_id.clone(),
            directory: self.directory.to_string_lossy().into_owned(),
            manifest_path: manifest_path.to_string_lossy().into_owned(),
            frame_pattern: FRAME_PATTERN.to_owned(),
            frame_count: self.frames.len().try_into().map_err(|_| {
                RecordingArtifactError::Invalid("recording has too many frames".to_owned())
            })?,
            width,
            height,
            frames_per_second: self.frames_per_second,
            started_at_ms,
            ended_at_ms,
            manifest_sha256: hash_bytes_sha256(&manifest_bytes),
        };
        self.committed = true;
        Ok(artifact)
    }
}

impl Drop for RecordingArtifactWriter {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

fn valid_recording_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
