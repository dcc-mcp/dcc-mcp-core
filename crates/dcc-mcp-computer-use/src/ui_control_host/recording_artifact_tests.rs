use std::fs;

use dcc_mcp_ui_control::host_protocol::UiControlTarget;
use serde_json::Value;

use super::recording_artifact::RecordingArtifactWriter;

fn target() -> UiControlTarget {
    UiControlTarget {
        process_id: 42,
        window_handle: 0x1234,
        window_title: "Eclipse Swarm".to_owned(),
    }
}

#[test]
fn recording_manifest_is_committed_last_with_per_frame_hashes() {
    let root = tempfile::tempdir().expect("create recording root");
    let mut writer =
        RecordingArtifactWriter::create_in(root.path(), "recording-1", target(), 30, 92)
            .expect("create writer");

    writer
        .write_frame(0, 1_000, 2, 1, b"jpeg-frame-zero")
        .expect("write first frame");
    writer
        .write_frame(1, 1_034, 2, 1, b"jpeg-frame-one")
        .expect("write second frame");
    assert!(!writer.directory().join("manifest.json").exists());

    let artifact = writer.finish(1_067).expect("finish recording");
    assert_eq!(artifact.recording_id, "recording-1");
    assert_eq!(artifact.frame_count, 2);
    assert_eq!(artifact.width, 2);
    assert_eq!(artifact.height, 1);
    assert_eq!(artifact.frames_per_second, 30);
    assert_eq!(artifact.started_at_ms, 1_000);
    assert_eq!(artifact.ended_at_ms, 1_067);
    assert_eq!(artifact.manifest_sha256.len(), 64);

    let manifest_bytes = fs::read(&artifact.manifest_path).expect("read manifest");
    assert_eq!(
        dcc_mcp_artefact::hash_bytes_sha256(&manifest_bytes),
        artifact.manifest_sha256
    );
    let manifest: Value = serde_json::from_slice(&manifest_bytes).expect("parse manifest");
    assert_eq!(manifest["target"]["process_id"], 42);
    assert_eq!(manifest["target"]["window_handle"], 0x1234);
    assert_eq!(manifest["encoding"]["format"], "jpeg_sequence");
    assert_eq!(manifest["encoding"]["jpeg_quality"], 92);
    assert_eq!(manifest["frames"].as_array().unwrap().len(), 2);
    for (index, frame) in manifest["frames"].as_array().unwrap().iter().enumerate() {
        assert_eq!(frame["index"], index as u64);
        assert_eq!(frame["sha256"].as_str().unwrap().len(), 64);
        assert!(
            writer
                .directory()
                .join(frame["path"].as_str().unwrap())
                .is_file()
        );
    }
}

#[test]
fn incomplete_or_inconsistent_recordings_are_removed_on_drop() {
    let root = tempfile::tempdir().expect("create recording root");
    let directory = root.path().join("recording-failed");
    {
        let mut writer =
            RecordingArtifactWriter::create_in(root.path(), "recording-failed", target(), 60, 90)
                .expect("create writer");
        writer
            .write_frame(0, 2_000, 1280, 720, b"first")
            .expect("write first frame");
        assert!(
            writer
                .write_frame(1, 2_016, 1920, 1080, b"changed-size")
                .is_err()
        );
    }
    assert!(!directory.exists());
}

#[test]
fn recording_ids_cannot_escape_the_host_owned_root() {
    let root = tempfile::tempdir().expect("create recording root");
    assert!(
        RecordingArtifactWriter::create_in(root.path(), "../escape", target(), 30, 92).is_err()
    );
    assert!(!root.path().parent().unwrap().join("escape").exists());
}
