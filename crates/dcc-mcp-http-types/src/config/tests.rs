use super::*;
use std::collections::HashMap;
use std::path::PathBuf;

// ── ServerSpawnMode ────────────────────────────────────────────────

#[test]
fn server_spawn_mode_defaults_to_ambient() {
    assert_eq!(ServerSpawnMode::default(), ServerSpawnMode::Ambient);
}

#[test]
fn server_spawn_mode_wire_is_snake_case() {
    // `ambient` / `dedicated` is the wire form the Python binding
    // and env-var plumbing round-trip. Pin it so a future derive
    // tweak cannot silently break downstream consumers.
    assert_eq!(
        serde_json::to_string(&ServerSpawnMode::Ambient).unwrap(),
        "\"ambient\""
    );
    assert_eq!(
        serde_json::to_string(&ServerSpawnMode::Dedicated).unwrap(),
        "\"dedicated\""
    );

    let back: ServerSpawnMode = serde_json::from_str("\"dedicated\"").unwrap();
    assert_eq!(back, ServerSpawnMode::Dedicated);
}

// ── JobRecoveryPolicy ──────────────────────────────────────────────

/// Issue #567: the policy enum defaults to `Drop` so existing callers
/// inherit today's behaviour without touching their config.
#[test]
fn job_recovery_default_is_drop() {
    assert_eq!(JobRecoveryPolicy::default(), JobRecoveryPolicy::Drop);
}

/// Issue #567: the wire identifier round-trips to the same shape the
/// Python binding exposes.
#[test]
fn job_recovery_as_str_matches_wire() {
    assert_eq!(JobRecoveryPolicy::Drop.as_str(), "drop");
    assert_eq!(JobRecoveryPolicy::Requeue.as_str(), "requeue");
}

/// Issue #567: env-var plumbing (`DCC_MCP_*_JOB_RECOVERY=Requeue`) and
/// the Python setter share the same case-insensitive parser.
#[test]
fn job_recovery_parse_is_case_insensitive() {
    for raw in ["drop", "Drop", "DROP", "  drop  "] {
        assert_eq!(JobRecoveryPolicy::parse(raw), Ok(JobRecoveryPolicy::Drop));
    }
    for raw in ["requeue", "Requeue", "REQUEUE"] {
        assert_eq!(
            JobRecoveryPolicy::parse(raw),
            Ok(JobRecoveryPolicy::Requeue)
        );
    }
}

/// Issue #567: unknown policies surface a descriptive error that
/// names the rejected value and the accepted set.
#[test]
fn job_recovery_parse_rejects_unknown() {
    let err = JobRecoveryPolicy::parse("retry").unwrap_err();
    assert!(err.contains("retry"), "error message: {err}");
    assert!(err.contains("drop"), "error message: {err}");
    assert!(err.contains("requeue"), "error message: {err}");
}

/// The snake_case JSON form matches the CLI / env-var string form,
/// so operators can read either serialisation interchangeably.
#[test]
fn job_recovery_wire_is_snake_case() {
    assert_eq!(
        serde_json::to_string(&JobRecoveryPolicy::Drop).unwrap(),
        "\"drop\""
    );
    assert_eq!(
        serde_json::to_string(&JobRecoveryPolicy::Requeue).unwrap(),
        "\"requeue\""
    );

    let back: JobRecoveryPolicy = serde_json::from_str("\"requeue\"").unwrap();
    assert_eq!(back, JobRecoveryPolicy::Requeue);
}

// ── JobConfig ──────────────────────────────────────────────────────

#[test]
fn job_config_default_is_in_memory_with_drop_policy() {
    let cfg = JobConfig::default();
    assert!(cfg.job_storage_path.is_none());
    assert_eq!(cfg.job_recovery, JobRecoveryPolicy::Drop);
}

#[test]
fn job_config_serialises_skip_none_storage() {
    // `job_storage_path: None` is the default — keeping it out of
    // the JSON serialisation keeps round-tripped configs compact
    // and matches the CLI default (no `--job-storage-path` flag).
    let cfg = JobConfig::default();
    let s = serde_json::to_string(&cfg).unwrap();
    assert!(!s.contains("job_storage_path"), "got: {s}");
    assert!(s.contains("\"job_recovery\":\"drop\""), "got: {s}");
}

#[test]
fn job_config_round_trips_with_storage_path() {
    let cfg = JobConfig {
        job_storage_path: Some(PathBuf::from("/var/lib/dcc/jobs.sqlite")),
        job_recovery: JobRecoveryPolicy::Requeue,
    };
    let s = serde_json::to_string(&cfg).unwrap();
    let back: JobConfig = serde_json::from_str(&s).unwrap();
    assert_eq!(back.job_storage_path, cfg.job_storage_path);
    assert_eq!(back.job_recovery, cfg.job_recovery);
}

#[test]
fn job_config_accepts_minimal_body() {
    // Operators frequently send a 2-key partial in env-var
    // configs. Both fields default, so a `{}` body must still
    // deserialise to the documented defaults — anything else
    // would surprise CLI / Python plumbing.
    let cfg: JobConfig = serde_json::from_str("{}").unwrap();
    assert!(cfg.job_storage_path.is_none());
    assert_eq!(cfg.job_recovery, JobRecoveryPolicy::Drop);
}

// ── WorkflowConfig ─────────────────────────────────────────────────

#[test]
fn workflow_config_default_disables_both_subsystems() {
    // Pristine boot must surface only the minimal MCP tools, so
    // both opt-in switches default to `false`. Operators flip
    // them on consciously when they are ready to pay the
    // workflow / scheduler runtime cost.
    let cfg = WorkflowConfig::default();
    assert!(!cfg.enable_workflows);
    assert!(!cfg.enable_scheduler);
    assert!(cfg.schedules_dir.is_none());
}

#[test]
fn workflow_config_round_trips() {
    let cfg = WorkflowConfig {
        enable_workflows: true,
        enable_scheduler: true,
        schedules_dir: Some(PathBuf::from("/etc/dcc/schedules")),
    };
    let s = serde_json::to_string(&cfg).unwrap();
    let back: WorkflowConfig = serde_json::from_str(&s).unwrap();
    assert_eq!(back.enable_workflows, cfg.enable_workflows);
    assert_eq!(back.enable_scheduler, cfg.enable_scheduler);
    assert_eq!(back.schedules_dir, cfg.schedules_dir);
}

#[test]
fn workflow_config_skip_none_schedules_dir() {
    let cfg = WorkflowConfig::default();
    let s = serde_json::to_string(&cfg).unwrap();
    assert!(!s.contains("schedules_dir"), "got: {s}");
}

#[test]
fn workflow_config_accepts_minimal_body() {
    let cfg: WorkflowConfig = serde_json::from_str("{}").unwrap();
    assert!(!cfg.enable_workflows);
    assert!(!cfg.enable_scheduler);
    assert!(cfg.schedules_dir.is_none());
}

// ── TelemetryConfig ────────────────────────────────────────────────

#[test]
fn telemetry_config_default_is_disabled_and_unauth() {
    // Pristine boot must NOT expose `/metrics` and must NOT
    // accept arbitrary scrapers — operators flip both knobs on
    // consciously.
    let cfg = TelemetryConfig::default();
    assert!(!cfg.enable_prometheus);
    assert!(cfg.prometheus_basic_auth.is_none());
}

#[test]
fn telemetry_config_round_trips_with_basic_auth() {
    let cfg = TelemetryConfig {
        enable_prometheus: true,
        prometheus_basic_auth: Some(("scraper".into(), "s3cret".into())),
    };
    let s = serde_json::to_string(&cfg).unwrap();
    let back: TelemetryConfig = serde_json::from_str(&s).unwrap();
    assert!(back.enable_prometheus);
    assert_eq!(
        back.prometheus_basic_auth,
        Some(("scraper".to_owned(), "s3cret".to_owned()))
    );
}

#[test]
fn telemetry_config_skips_none_basic_auth() {
    let cfg = TelemetryConfig::default();
    let s = serde_json::to_string(&cfg).unwrap();
    // Default config must not leak `prometheus_basic_auth: null`
    // into the wire form — keeps env-var/config-file dumps tidy.
    assert!(!s.contains("prometheus_basic_auth"), "got: {s}");
}

#[test]
fn telemetry_config_accepts_minimal_body() {
    let cfg: TelemetryConfig = serde_json::from_str("{}").unwrap();
    assert!(!cfg.enable_prometheus);
    assert!(cfg.prometheus_basic_auth.is_none());
}

// ── FeatureFlags ───────────────────────────────────────────────────

/// Pin every default boolean of [`FeatureFlags`]. Most are `false`,
/// but `bare_tool_names`, `enable_resources`, `enable_prompts`,
/// and `enable_job_notifications` default to `true` because that
/// is the documented pre-#852 surface the wheel ships with.
/// A future change to any of these defaults must be conscious;
/// this test is the regression guard.
#[test]
fn feature_flags_default_matches_documented_pre_852_surface() {
    let f = FeatureFlags::default();
    assert!(!f.lazy_actions);
    assert!(f.bare_tool_names);
    assert!(f.enable_resources);
    assert!(f.enable_prompts);
    assert!(!f.enable_artefact_resources);
    assert!(f.enable_job_notifications);
    assert!(!f.shutdown_on_drop);
    assert!(!f.exclude_skill_stubs_from_tools_list);
    assert!(!f.exclude_group_stubs_from_tools_list);
    assert!(!f.standalone_main_thread_execution);
}

#[test]
fn feature_flags_round_trip() {
    let f = FeatureFlags::default();
    let s = serde_json::to_string(&f).unwrap();
    let back: FeatureFlags = serde_json::from_str(&s).unwrap();
    assert_eq!(back.lazy_actions, f.lazy_actions);
    assert_eq!(back.bare_tool_names, f.bare_tool_names);
    assert_eq!(back.enable_resources, f.enable_resources);
    assert_eq!(back.enable_prompts, f.enable_prompts);
    assert_eq!(back.enable_artefact_resources, f.enable_artefact_resources);
    assert_eq!(back.enable_job_notifications, f.enable_job_notifications);
    assert_eq!(back.shutdown_on_drop, f.shutdown_on_drop);
    assert_eq!(
        back.exclude_skill_stubs_from_tools_list,
        f.exclude_skill_stubs_from_tools_list
    );
    assert_eq!(
        back.exclude_group_stubs_from_tools_list,
        f.exclude_group_stubs_from_tools_list
    );
    assert_eq!(
        back.standalone_main_thread_execution,
        f.standalone_main_thread_execution
    );
}

#[test]
fn standalone_main_thread_execution_builder_opts_in() {
    let cfg = McpHttpConfig::default().with_standalone_main_thread_execution();
    assert!(cfg.standalone_main_thread_execution());
}

/// Critical contract: an empty `{}` body must deserialise into
/// the documented Default surface, NOT into "every flag is
/// `false`". The four `default = "default_true"` annotations
/// are what keep this guarantee — drop one and the wheel
/// silently regresses to a different `tools/list` shape.
#[test]
fn feature_flags_minimal_body_uses_per_field_defaults() {
    let f: FeatureFlags = serde_json::from_str("{}").unwrap();
    let d = FeatureFlags::default();
    assert_eq!(f.lazy_actions, d.lazy_actions);
    assert_eq!(f.bare_tool_names, d.bare_tool_names);
    assert_eq!(f.enable_resources, d.enable_resources);
    assert_eq!(f.enable_prompts, d.enable_prompts);
    assert_eq!(f.enable_artefact_resources, d.enable_artefact_resources);
    assert_eq!(f.enable_job_notifications, d.enable_job_notifications);
    assert_eq!(f.shutdown_on_drop, d.shutdown_on_drop);
    assert_eq!(
        f.exclude_skill_stubs_from_tools_list,
        d.exclude_skill_stubs_from_tools_list
    );
    assert_eq!(
        f.exclude_group_stubs_from_tools_list,
        d.exclude_group_stubs_from_tools_list
    );
}

#[test]
fn feature_flags_partial_body_inherits_other_defaults() {
    // Operators only flip `lazy_actions` on; every other knob
    // must keep its documented default.
    let f: FeatureFlags = serde_json::from_str(r#"{"lazy_actions": true}"#).unwrap();
    assert!(f.lazy_actions);
    // The defaults still hold for unmentioned fields:
    assert!(f.bare_tool_names);
    assert!(f.enable_resources);
    assert!(f.enable_prompts);
    assert!(f.enable_job_notifications);
    // And the `false`-by-default ones stay `false`:
    assert!(!f.enable_artefact_resources);
    assert!(!f.shutdown_on_drop);
}

// ── InstanceConfig ─────────────────────────────────────────────────

#[test]
fn instance_config_default_is_anonymous() {
    // A pristine InstanceConfig must reveal nothing about the
    // host adapter — every field is None / empty so that
    // `FileRegistry` rows from a misconfigured launcher do not
    // accidentally claim to be Maya / Blender / etc.
    let cfg = InstanceConfig::default();
    assert!(cfg.dcc_type.is_none());
    assert!(cfg.dcc_version.is_none());
    assert!(cfg.scene.is_none());
    assert!(cfg.instance_metadata.is_empty());
    assert!(cfg.declared_capabilities.is_empty());
}

#[test]
fn instance_config_round_trips() {
    let mut metadata = HashMap::new();
    metadata.insert("project".to_owned(), "shotpack".to_owned());
    metadata.insert("task".to_owned(), "lighting".to_owned());

    let cfg = InstanceConfig {
        dcc_type: Some("maya".into()),
        dcc_version: Some("2025.1".into()),
        scene: Some("/tmp/scene.ma".into()),
        instance_metadata: metadata.clone(),
        declared_capabilities: vec!["usd".into(), "scene.mutate".into()],
    };
    let s = serde_json::to_string(&cfg).unwrap();
    let back: InstanceConfig = serde_json::from_str(&s).unwrap();
    assert_eq!(back.dcc_type, cfg.dcc_type);
    assert_eq!(back.dcc_version, cfg.dcc_version);
    assert_eq!(back.scene, cfg.scene);
    assert_eq!(back.instance_metadata, metadata);
    assert_eq!(back.declared_capabilities, cfg.declared_capabilities);
}

/// Every optional / collection field carries
/// `skip_serializing_if = ...` so a pristine `InstanceConfig`
/// serialises to the literal `"{}"`. Pin this so a future field
/// addition does not silently bloat config dumps with `null`s
/// and empty arrays.
#[test]
fn instance_config_default_serialises_empty_object() {
    let cfg = InstanceConfig::default();
    let s = serde_json::to_string(&cfg).unwrap();
    assert_eq!(s, "{}", "default config must serialise to empty object");
}

#[test]
fn instance_config_accepts_minimal_body() {
    // `{}` must deserialise to defaults. Operators routinely
    // boot a server without any of these fields set, then patch
    // the registry row in via subsequent calls; if `{}` failed
    // to deserialise, the boot sequence would break.
    let cfg: InstanceConfig = serde_json::from_str("{}").unwrap();
    assert!(cfg.dcc_type.is_none());
    assert!(cfg.declared_capabilities.is_empty());
}
