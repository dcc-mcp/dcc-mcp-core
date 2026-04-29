//! Unit tests for [`super`].

use super::writer::{CalendarDate, prune_old};
use super::*;
use std::io::Write;
use std::path::PathBuf;

fn tmp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "dcc-mcp-file-logging-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn parses_rotation_policies() {
    assert_eq!(RotationPolicy::parse("size").unwrap(), RotationPolicy::Size);
    assert_eq!(
        RotationPolicy::parse("DAILY").unwrap(),
        RotationPolicy::Daily
    );
    assert_eq!(RotationPolicy::parse("both").unwrap(), RotationPolicy::Both);
    assert!(RotationPolicy::parse("nonsense").is_err());
}

#[test]
fn size_rotation_creates_rolled_file() {
    let dir = tmp_dir("size");
    let cfg = FileLoggingConfig {
        directory: Some(dir.clone()),
        file_name_prefix: "unit".to_string(),
        max_size_bytes: 32,
        max_files: 3,
        rotation: RotationPolicy::Size,
        include_console: true,
        retention_days: 0,
        max_total_size_mb: 0,
    };
    let mut writer = RollingFileWriter::new(&cfg).unwrap();

    // First write below threshold — no rotation.
    writer.write_all(b"hello\n").unwrap();
    // Second write pushes us past 32 bytes.
    writer.write_all(&[b'x'; 64]).unwrap();
    writer.flush().unwrap();
    drop(writer);

    let entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.starts_with("unit.") && n.ends_with(".log"))
                .unwrap_or(false)
        })
        .collect();

    assert!(
        entries.len() >= 2,
        "expected rotated + current, got {entries:?}"
    );
}

#[test]
fn retention_caps_rolled_files() {
    let dir = tmp_dir("retain");
    // Seed 6 bogus rolled files, then prune to max_files = 2.
    for i in 0..6 {
        let name = format!("unit.2020010{i}T000000.log");
        std::fs::write(dir.join(&name), format!("content {i}")).unwrap();
    }
    // Plus one "current" file so prune_old keeps it.
    std::fs::write(
        dir.join(format!(
            "unit.{}.log",
            CalendarDate::today_local().as_basename()
        )),
        b"current",
    )
    .unwrap();

    prune_old(&dir, "unit", 2);

    let rolled: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("unit.") && n.ends_with(".log") && n.contains('T')
            // timestamped = rolled
            {
                Some(n)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(rolled.len(), 2, "kept: {rolled:?}");
}

#[test]
fn init_and_shutdown_are_idempotent() {
    let dir = tmp_dir("install");
    let cfg = FileLoggingConfig {
        directory: Some(dir.clone()),
        file_name_prefix: "install".to_string(),
        max_size_bytes: 1024,
        max_files: 2,
        rotation: RotationPolicy::Both,
        include_console: true,
        retention_days: 0,
        max_total_size_mb: 0,
    };

    let resolved = init_file_logging(cfg.clone()).unwrap();
    assert_eq!(resolved, dir);

    // Swap — should not panic.
    let resolved2 = init_file_logging(cfg).unwrap();
    assert_eq!(resolved2, dir);

    shutdown_file_logging().unwrap();
    // Second shutdown is a no-op.
    shutdown_file_logging().unwrap();
}
