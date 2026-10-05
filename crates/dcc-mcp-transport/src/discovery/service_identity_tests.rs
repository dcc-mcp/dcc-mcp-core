//! Regression tests for the identity / lineage envelope (RFC-0007 §3.2).
//!
//! The hard constraint these pin: rows written **without** the envelope must
//! behave exactly as they did before it existed — same parse, same wire shape,
//! no new keys. Rows written **with** it must survive `services.json`.

use super::{
    LAUNCH_ID_ENV_VAR, ServiceRole, launch_id_from_env_value, process_launch_id,
    system_time_to_unix_secs,
};
use crate::discovery::file_registry::FileRegistry;
use crate::discovery::types::{ServiceEntry, ServiceKey};
use std::time::{Duration, SystemTime};

/// A row as an adapter built before this envelope wrote it: no `launch_id`,
/// no `parent_pid`, no `role`, no `started_at`.
fn legacy_maya_row() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "dcc_type": "maya",
        "instance_id": "00000000-0000-0000-0000-000000000001",
        "host": "127.0.0.1",
        "port": 18812,
        "version": "2024.2",
        "adapter_version": "0.3.0",
        "adapter_dcc": "maya",
        "scene": "/shots/character.ma",
        "documents": [],
        "pid": 4321,
        "host_pid": 4320,
        "display_name": "Maya-Rigging",
        "metadata": {"dcc_mcp_instance_type": "gui"},
        "registered_at": 1712345678.5,
        "last_heartbeat": 1712345690.5,
        "status": "available",
    })
}

/// Legacy multi-document row from a second host family (Photoshop), written by
/// a Python bridge plugin with float timestamps and no envelope.
fn legacy_photoshop_row() -> serde_json::Value {
    serde_json::json!({
        "dcc_type": "photoshop",
        "instance_id": "00000000-0000-0000-0000-000000000002",
        "host": "127.0.0.1",
        "port": 18813,
        "scene": "/jobs/poster.psd",
        "documents": ["/jobs/poster.psd", "/jobs/hero.psd"],
        "pid": 9001,
        "display_name": "PS-Marketing",
        "registered_at": 1712345678,
        "last_heartbeat": 1712345699,
        "status": "busy",
    })
}

fn parse_row(row: serde_json::Value) -> ServiceEntry {
    serde_json::from_value(row).expect("legacy row must deserialize")
}

fn envelope_keys() -> [&'static str; 4] {
    ["launch_id", "parent_pid", "role", "started_at"]
}

#[test]
fn legacy_row_deserializes_with_unknown_envelope() {
    let entry = parse_row(legacy_maya_row());

    assert_eq!(entry.launch_id, None);
    assert_eq!(entry.parent_pid, None);
    assert_eq!(entry.role, None);
    assert_eq!(entry.started_at, None);
    // Everything the row *did* carry is untouched.
    assert_eq!(entry.dcc_type, "maya");
    assert_eq!(entry.pid, Some(4321));
    assert_eq!(entry.host_pid, Some(4320));
    assert_eq!(entry.display_name.as_deref(), Some("Maya-Rigging"));
    assert_eq!(
        entry
            .metadata
            .get("dcc_mcp_instance_type")
            .map(String::as_str),
        Some("gui")
    );
    let registered = entry
        .registered_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    assert_eq!(registered.as_secs(), 1_712_345_678);
    assert_eq!(registered.subsec_nanos(), 500_000_000);
}

#[test]
fn legacy_row_keeps_its_wire_shape_after_a_read_write_cycle() {
    // The regression that matters: reading a legacy row and writing it back
    // must not grow the payload. `skip_serializing_if` keeps the four keys out.
    let entry = parse_row(legacy_photoshop_row());
    let written = serde_json::to_value(&entry).unwrap();

    for key in envelope_keys() {
        assert!(
            written.get(key).is_none(),
            "legacy row must not gain `{key}`: {written}"
        );
    }
    // Documents / scene survive unchanged.
    assert_eq!(
        written["documents"],
        serde_json::json!(["/jobs/poster.psd", "/jobs/hero.psd"])
    );
    assert_eq!(written["scene"], serde_json::json!("/jobs/poster.psd"));
}

#[test]
fn legacy_rows_are_never_the_same_launch() {
    let maya = parse_row(legacy_maya_row());
    let photoshop = parse_row(legacy_photoshop_row());
    let other_maya = parse_row(legacy_maya_row());

    // Absence is *unknown*, not evidence of a shared launch — collapsing on
    // `None` would merge unrelated sessions from pre-envelope adapters.
    assert!(!maya.same_launch(&other_maya));
    assert!(!maya.same_launch(&photoshop));
}

#[test]
fn constructors_leave_the_envelope_unset() {
    for entry in [
        ServiceEntry::new("maya", "127.0.0.1", 18812),
        ServiceEntry::with_address(
            "maya",
            crate::ipc::TransportAddress::named_pipe("dcc-mcp-maya-1"),
        ),
    ] {
        assert_eq!(entry.launch_id, None);
        assert_eq!(entry.parent_pid, None);
        assert_eq!(entry.role, None);
        assert_eq!(entry.started_at, None);

        let json = serde_json::to_string(&entry).unwrap();
        for key in envelope_keys() {
            assert!(
                !json.contains(&format!("\"{key}\"")),
                "unset `{key}` must be skipped: {json}"
            );
        }
    }
}

#[test]
fn builders_stamp_the_envelope_and_round_trip() {
    let started_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_712_345_600);
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812)
        .with_launch_id("launch-7f3c")
        .with_parent_pid(4242)
        .with_role(ServiceRole::Host)
        .with_started_at(started_at);

    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("\"launch_id\":\"launch-7f3c\""), "{json}");
    assert!(json.contains("\"parent_pid\":4242"), "{json}");
    assert!(json.contains("\"role\":\"host\""), "{json}");

    let parsed: ServiceEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.launch_id.as_deref(), Some("launch-7f3c"));
    assert_eq!(parsed.parent_pid, Some(4242));
    assert_eq!(parsed.role, Some(ServiceRole::Host));
    assert_eq!(parsed.started_at, Some(started_at));
    assert_eq!(system_time_to_unix_secs(started_at), Some(1_712_345_600));
}

#[test]
fn role_accepts_strings_and_serializes_as_plain_lowercase_strings() {
    for (role, wire) in [
        (ServiceRole::Host, "host"),
        (ServiceRole::Launcher, "launcher"),
        (ServiceRole::Sidecar, "sidecar"),
    ] {
        let entry = ServiceEntry::new("blender", "127.0.0.1", 18765).with_role(wire);
        assert_eq!(entry.role, Some(role.clone()));
        assert_eq!(entry.role.as_ref().map(ServiceRole::as_str), Some(wire));

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(&format!("\"role\":\"{wire}\"")), "{json}");
    }
}

#[test]
fn unknown_role_survives_a_round_trip_verbatim() {
    // A newer producer publishing a role this build does not know must not be
    // rejected, and must not be rewritten to something lossy — only surrounding
    // whitespace is trimmed, the spelling is kept.
    let entry = ServiceEntry::new("blender", "127.0.0.1", 18765).with_role("Gateway-Sidecar");
    assert_eq!(
        entry.role,
        Some(ServiceRole::Custom("Gateway-Sidecar".to_string()))
    );

    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("\"role\":\"Gateway-Sidecar\""), "{json}");

    let parsed: ServiceEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed.role.as_ref().map(ServiceRole::as_str),
        Some("Gateway-Sidecar")
    );
    assert_eq!(parsed.role, entry.role);
}

#[test]
fn known_roles_match_case_insensitively() {
    // Matching is lenient; the stored wire value is the canonical one.
    for raw in ["Host", " HOST ", "host"] {
        let role = ServiceRole::from(raw);
        assert_eq!(role, ServiceRole::Host);
        assert_eq!(role.as_str(), "host");
    }
    assert_eq!(ServiceRole::from("  SiDeCaR"), ServiceRole::Sidecar);
    assert_eq!(ServiceRole::from("Launcher"), ServiceRole::Launcher);
}

#[test]
fn unparsable_started_at_degrades_to_none_instead_of_failing_the_row() {
    // A row that fails to deserialize would quarantine the whole
    // `services.json`, dropping every registered instance. `started_at` is
    // lineage metadata nobody routes on, so a bad value must not do that.
    let mut row = legacy_maya_row();
    row["started_at"] = serde_json::json!("not-a-timestamp");

    let entry = parse_row(row);
    assert_eq!(entry.started_at, None);
    assert_eq!(entry.dcc_type, "maya");
    assert_eq!(entry.pid, Some(4321));
}

#[test]
fn started_at_accepts_float_and_struct_timestamp_shapes() {
    let mut row = legacy_photoshop_row();
    row["started_at"] = serde_json::json!(1_712_345_678.5);
    let float_entry = parse_row(row);
    assert_eq!(
        float_entry.started_at.map(system_time_to_unix_secs),
        Some(Some(1_712_345_678))
    );

    let mut row = legacy_photoshop_row();
    row["started_at"] =
        serde_json::json!({"secs_since_epoch": 1_712_345_600, "nanos_since_epoch": 0});
    let struct_entry = parse_row(row);
    assert_eq!(
        struct_entry.started_at.map(system_time_to_unix_secs),
        Some(Some(1_712_345_600))
    );
}

#[test]
fn same_launch_pairs_rows_from_one_process_tree() {
    // A launcher and the sidecar it spawned: different rows, one launch.
    let launcher = ServiceEntry::new("blender", "127.0.0.1", 18765)
        .with_launch_id("launch-abc")
        .with_role(ServiceRole::Launcher);
    let sidecar = ServiceEntry::new("blender", "127.0.0.1", 18766)
        .with_launch_id("launch-abc")
        .with_parent_pid(std::process::id())
        .with_role(ServiceRole::Sidecar);
    let other_launch = ServiceEntry::new("blender", "127.0.0.1", 18767)
        .with_launch_id("launch-def")
        .with_role(ServiceRole::Sidecar);
    let unknown = ServiceEntry::new("blender", "127.0.0.1", 18768);

    assert!(launcher.same_launch(&sidecar));
    assert!(sidecar.same_launch(&launcher));
    assert!(!launcher.same_launch(&other_launch));
    assert!(!launcher.same_launch(&unknown));
    assert!(!unknown.same_launch(&unknown));
}

#[test]
fn launch_id_env_value_ignores_blank_values() {
    assert_eq!(launch_id_from_env_value(None), None);
    assert_eq!(launch_id_from_env_value(Some("")), None);
    assert_eq!(launch_id_from_env_value(Some("   ")), None);
    assert_eq!(
        launch_id_from_env_value(Some("  launch-9  ")).as_deref(),
        Some("launch-9")
    );
    // The variable name is part of the producer contract (launcher → children).
    assert_eq!(LAUNCH_ID_ENV_VAR, "DCC_MCP_LAUNCH_ID");
}

#[test]
fn process_launch_id_is_stable_for_the_whole_process() {
    // Rows registered by one process share the id even when no launcher set
    // the environment variable.
    let first = process_launch_id();
    let second = process_launch_id();
    assert!(!first.is_empty());
    assert_eq!(first, second);
}

#[test]
fn envelope_survives_a_file_registry_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let started_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_712_345_600);
    let entry = ServiceEntry::new("houdini", "127.0.0.1", 18820)
        .with_launch_id("launch-registry")
        .with_parent_pid(777)
        .with_role("sidecar")
        .with_started_at(started_at);

    let key = entry.key();
    FileRegistry::new(dir.path())
        .unwrap()
        .register(entry)
        .unwrap();

    // Visible in `services.json` — one of the three acceptance criteria.
    let raw = std::fs::read_to_string(dir.path().join("services.json")).unwrap();
    let on_disk: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let row = &on_disk[0];
    assert_eq!(row["launch_id"], serde_json::json!("launch-registry"));
    assert_eq!(row["parent_pid"], serde_json::json!(777));
    assert_eq!(row["role"], serde_json::json!("sidecar"));
    assert_eq!(
        row["started_at"]["secs_since_epoch"],
        serde_json::json!(1_712_345_600)
    );

    let read_back = FileRegistry::new(dir.path())
        .unwrap()
        .get(&key)
        .expect("entry must be readable");
    assert_eq!(read_back.launch_id.as_deref(), Some("launch-registry"));
    assert_eq!(read_back.parent_pid, Some(777));
    assert_eq!(read_back.role, Some(ServiceRole::Sidecar));
    assert_eq!(read_back.started_at, Some(started_at));
}

#[test]
fn legacy_services_json_row_reads_without_the_envelope() {
    let dir = tempfile::tempdir().unwrap();
    // Hand-written file the way a pre-envelope adapter left it: a bare JSON
    // array, the shape `parse_registry_entries` reads.
    let maya = legacy_maya_row();
    let photoshop = legacy_photoshop_row();
    std::fs::write(
        dir.path().join("services.json"),
        serde_json::to_string(&serde_json::json!([maya, photoshop])).unwrap(),
    )
    .unwrap();

    let registry = FileRegistry::new(dir.path()).unwrap();
    let entries = registry.list_all();
    assert_eq!(entries.len(), 2);

    for entry in &entries {
        assert_eq!(entry.launch_id, None);
        assert_eq!(entry.parent_pid, None);
        assert_eq!(entry.role, None);
        assert_eq!(entry.started_at, None);
    }

    let key = ServiceKey {
        dcc_type: "maya".to_string(),
        instance_id: "00000000-0000-0000-0000-000000000001".parse().unwrap(),
    };
    let maya_entry = registry.get(&key).expect("maya row must be readable");
    assert_eq!(maya_entry.display_name.as_deref(), Some("Maya-Rigging"));
    assert!(!maya_entry.same_launch(&entries[1]));
}
