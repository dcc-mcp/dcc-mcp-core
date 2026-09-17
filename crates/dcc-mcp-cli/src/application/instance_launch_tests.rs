//! Tests for the project-bound start-instance lifecycle.

use super::*;
use crate::domain::start_instance::{
    LAUNCH_PLAN_DIR_NAME, PLACEHOLDER_EXECUTABLE, PLACEHOLDER_PROJECT,
    PROJECT_LAUNCH_PLAN_RELATIVE_PATH,
};
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::DispatchStatus;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// ── Fixtures ───────────────────────────────────────────────────────────────

/// A cross-platform executable that exits immediately. Used to prove the launch
/// stage really spawns a process without needing a DCC installed.
fn noop_executable() -> (std::path::PathBuf, &'static str) {
    if cfg!(windows) {
        let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        (std::path::PathBuf::from(comspec), "/c")
    } else {
        (std::path::PathBuf::from("/bin/sh"), "-c")
    }
}

fn write_project_plan(project: &Path, executable: &Path, argv: &[String]) {
    let plan_dir = project.join(".dcc-mcp");
    std::fs::create_dir_all(&plan_dir).unwrap();
    let document = serde_json::json!({
        "schema_version": 1,
        "dcc_type": "unity",
        "executable": executable.display().to_string(),
        "argv": argv,
        "version": "2022.3.10f1",
        "project_markers": ["ProjectSettings/ProjectVersion.txt"],
    });
    std::fs::write(
        plan_dir.join("launch-plan.json"),
        serde_json::to_string(&document).unwrap(),
    )
    .unwrap();
}

/// A project directory carrying its adapter markers and a validated launch plan.
fn project_with_plan(root: &Path, name: &str, argv: &[String]) -> std::path::PathBuf {
    let project = root.join(name);
    std::fs::create_dir_all(project.join("ProjectSettings")).unwrap();
    std::fs::write(
        project.join("ProjectSettings").join("ProjectVersion.txt"),
        b"m_EditorVersion: 2022.3.10f1",
    )
    .unwrap();
    let (executable, _) = noop_executable();
    write_project_plan(&project, &executable, argv);
    project
}

fn request(project: &Path, registry: &Path, timeout_ms: u64) -> StartInstanceRequest {
    StartInstanceRequest {
        dcc_type: "unity".to_string(),
        project: project.to_path_buf(),
        launch_plan: None,
        version: None,
        wait_ready: false,
        timeout: Duration::from_millis(timeout_ms),
        interval: Duration::from_millis(50),
        required: Vec::new(),
        authorized: true,
        dry_run: false,
        registry_dir: registry.to_path_buf(),
        instance_id: None,
    }
}

/// One shared registry handle per directory.
///
/// `FileRegistry` holds an exclusive lock per registered row; dropping the
/// handle makes the row look dead to the next reader, so a fixture that
/// registers and returns would silently unregister the instance it just
/// created. Tests keep the handle for the lifetime of the process.
fn registry_handle(dir: &Path) -> Arc<FileRegistry> {
    static HANDLES: OnceLock<Mutex<HashMap<std::path::PathBuf, Arc<FileRegistry>>>> =
        OnceLock::new();
    let mut handles = HANDLES.get_or_init(Default::default).lock().unwrap();
    handles
        .entry(dir.to_path_buf())
        .or_insert_with(|| Arc::new(FileRegistry::new(dir.to_path_buf()).unwrap()))
        .clone()
}

fn register(dir: &Path, dcc_type: &str, port: u16, metadata: &[(&str, &str)]) -> ServiceEntry {
    let registry = registry_handle(dir);
    let mut entry = ServiceEntry::new(dcc_type, "127.0.0.1", port);
    entry.metadata.insert(
        "dispatch_status".to_string(),
        DispatchStatus::Ready.to_string(),
    );
    for (key, value) in metadata {
        entry
            .metadata
            .insert((*key).to_string(), (*value).to_string());
    }
    registry.register(entry.clone()).unwrap();
    entry
}

fn argv_for(executable: &Path, flag: &str) -> Vec<String> {
    vec![
        PLACEHOLDER_EXECUTABLE.to_string(),
        flag.to_string(),
        "exit 0".to_string(),
        PLACEHOLDER_PROJECT.to_string(),
    ]
    .into_iter()
    .map(|token| token.replace(PLACEHOLDER_EXECUTABLE, &executable.display().to_string()))
    .collect()
}

// ── Stage: resolve plan ────────────────────────────────────────────────────

#[tokio::test]
async fn no_published_plan_fails_closed_with_launch_plan_missing() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("MyProject");
    std::fs::create_dir_all(&project).unwrap();
    let registry = dir.path().join("registry");

    let value = start_instance(request(&project, &registry, 200))
        .await
        .unwrap();

    assert_eq!(value["ok"], false);
    assert_eq!(value["blocking_state"], "launch_plan_missing");
    assert_eq!(value["stage"], "resolve_plan");
    assert_eq!(value["next_action"]["id"], "install_adapter");
    assert!(
        value["diagnostics"]["searched"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| {
                path.as_str()
                    .unwrap()
                    .replace('\\', "/")
                    .contains(PROJECT_LAUNCH_PLAN_RELATIVE_PATH)
            })
    );
}

#[tokio::test]
async fn project_path_must_exist() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("Missing");
    let registry = dir.path().join("registry");

    let value = start_instance(request(&project, &registry, 200))
        .await
        .unwrap();

    assert_eq!(value["ok"], false);
    assert_eq!(value["blocking_state"], "project_not_found");
    assert_eq!(value["stage"], "project_binding");
}

#[tokio::test]
async fn missing_project_marker_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("WrongProject");
    std::fs::create_dir_all(&project).unwrap();
    let (executable, flag) = noop_executable();
    write_project_plan(&project, &executable, &argv_for(&executable, flag));
    let registry = dir.path().join("registry");

    let value = start_instance(request(&project, &registry, 200))
        .await
        .unwrap();

    assert_eq!(value["ok"], false);
    assert_eq!(value["blocking_state"], "project_marker_missing");
    assert_eq!(
        value["diagnostics"]["missing_marker"],
        "ProjectSettings/ProjectVersion.txt"
    );
}

#[tokio::test]
async fn dry_run_reports_the_resolved_plan_without_launching() {
    let dir = tempfile::tempdir().unwrap();
    let (executable, flag) = noop_executable();
    let project = project_with_plan(dir.path(), "MyProject", &argv_for(&executable, flag));
    let registry = dir.path().join("registry");

    let mut request = request(&project, &registry, 200);
    request.dry_run = true;
    request.authorized = false;
    let value = start_instance(request).await.unwrap();

    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["would_launch"], true);
    assert_eq!(value["launch_plan"]["source"], "project_receipt");
    assert_eq!(
        value["launch_plan"]["executable"],
        executable.display().to_string()
    );
    assert_eq!(value["next_action"]["id"], "authorize_launch");
    // Nothing may be launched on a dry run, so no operation is recorded.
    assert!(
        LifecycleStore::new(&registry)
            .find_owned("unity", &project)
            .unwrap()
            .is_none()
    );
}

// ── Stage: authorization ───────────────────────────────────────────────────

#[tokio::test]
async fn launching_a_gui_host_requires_authorization() {
    let dir = tempfile::tempdir().unwrap();
    let (executable, flag) = noop_executable();
    let project = project_with_plan(dir.path(), "MyProject", &argv_for(&executable, flag));
    let registry = dir.path().join("registry");

    let mut request = request(&project, &registry, 200);
    request.authorized = false;
    let value = start_instance(request).await.unwrap();

    assert_eq!(value["ok"], false);
    assert_eq!(value["blocking_state"], "authorization_required");
    assert_eq!(value["stage"], "authorization");
    assert_eq!(value["next_action"]["id"], "authorize_launch");
    assert!(
        LifecycleStore::new(&registry)
            .find_owned("unity", &project)
            .unwrap()
            .is_none()
    );
}

// ── Stage: launch + registration ───────────────────────────────────────────

#[tokio::test]
async fn launch_is_reported_and_registration_timeout_is_structured() {
    let dir = tempfile::tempdir().unwrap();
    let (executable, flag) = noop_executable();
    let project = project_with_plan(dir.path(), "MyProject", &argv_for(&executable, flag));
    let registry = dir.path().join("registry");

    let value = start_instance(request(&project, &registry, 300))
        .await
        .unwrap();

    assert_eq!(value["ok"], false);
    assert_eq!(value["blocking_state"], "timeout");
    assert_eq!(value["timeout_stage"], "registration");
    assert_eq!(value["next_action"]["id"], "retry_with_longer_timeout");
    assert!(value["pid"].as_u64().is_some(), "a pid must be reported");

    let store = LifecycleStore::new(&registry);
    let operation = store
        .find_owned("unity", &project)
        .unwrap()
        .expect("the launch must be recorded as an owned operation");
    assert!(operation.launched);
    assert!(operation.owned);
    assert_eq!(operation.instance_id, None);
}

#[tokio::test]
async fn launched_host_registers_and_is_selected_by_its_project_binding() {
    let dir = tempfile::tempdir().unwrap();
    let (executable, flag) = noop_executable();
    let project = project_with_plan(dir.path(), "MyProject", &argv_for(&executable, flag));
    let registry = dir.path().join("registry");

    // Simulate the launched host registering shortly after spawn, the way an
    // adapter stamps its project binding into the FileRegistry row.
    let spawn_registry = registry.clone();
    let spawn_project = project.display().to_string();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        register(
            &spawn_registry,
            "unity",
            18080,
            &[
                ("dcc_mcp_project", &spawn_project),
                ("window_handle", "0x0012ab"),
                ("compiling", "false"),
            ],
        );
    });

    let value = start_instance(request(&project, &registry, 3_000))
        .await
        .unwrap();

    assert_eq!(value["ok"], true);
    assert_eq!(value["launched"], true);
    assert_eq!(value["reused"], false);
    assert!(value["instance_id"].as_str().is_some());
    assert_eq!(value["window_handle"], "0x0012ab");
    assert_eq!(value["host_progress"]["compiling"], "false");
    assert_eq!(value["dispatch_state"], "ready");
    assert_eq!(value["stage"], "terminal");
    assert_eq!(value["blocking_state"], "none");
}

// ── Stage: reuse / convergence ─────────────────────────────────────────────

#[tokio::test]
async fn existing_project_bound_instance_is_reused_without_launching() {
    let dir = tempfile::tempdir().unwrap();
    let (executable, flag) = noop_executable();
    let project = project_with_plan(dir.path(), "MyProject", &argv_for(&executable, flag));
    let registry = dir.path().join("registry");
    register(
        &registry,
        "unity",
        18080,
        &[("dcc_mcp_project", &project.display().to_string())],
    );

    let value = start_instance(request(&project, &registry, 300))
        .await
        .unwrap();

    assert_eq!(value["ok"], true);
    assert_eq!(value["ready"], true);
    assert_eq!(value["reused"], true);
    assert_eq!(value["launched"], false);
    assert_eq!(value["owned"], false);
    assert_eq!(value["stage"], "terminal");
    assert_eq!(value["converged_on_operation_id"], serde_json::Value::Null);
}

#[tokio::test]
async fn second_identical_request_converges_on_the_same_instance() {
    let dir = tempfile::tempdir().unwrap();
    let (executable, flag) = noop_executable();
    let project = project_with_plan(dir.path(), "MyProject", &argv_for(&executable, flag));
    let registry = dir.path().join("registry");
    register(
        &registry,
        "unity",
        18080,
        &[("dcc_mcp_project", &project.display().to_string())],
    );

    let first = start_instance(request(&project, &registry, 300))
        .await
        .unwrap();
    let second = start_instance(request(&project, &registry, 300))
        .await
        .unwrap();

    assert_eq!(first["instance_id"], second["instance_id"]);
    assert!(first["operation_id"] != second["operation_id"]);
    assert_eq!(second["reused"], true);
    assert_eq!(second["launched"], false);
    assert_eq!(second["converged_on_operation_id"], first["operation_id"]);
}

#[tokio::test]
async fn ambiguous_project_binding_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (executable, flag) = noop_executable();
    let project = project_with_plan(dir.path(), "MyProject", &argv_for(&executable, flag));
    let registry = dir.path().join("registry");
    let binding = project.display().to_string();
    register(&registry, "unity", 18080, &[("dcc_mcp_project", &binding)]);
    register(&registry, "unity", 18081, &[("dcc_mcp_project", &binding)]);

    let value = start_instance(request(&project, &registry, 300))
        .await
        .unwrap();

    assert_eq!(value["ok"], false);
    assert_eq!(value["blocking_state"], "ambiguous_reuse");
    assert_eq!(value["stage"], "reuse");
    assert_eq!(value["next_action"]["id"], "select_exact_instance");
    assert_eq!(
        value["diagnostics"]["candidates"].as_array().unwrap().len(),
        2
    );
}

#[tokio::test]
async fn instance_bound_to_another_project_is_never_reused() {
    let dir = tempfile::tempdir().unwrap();
    let (executable, flag) = noop_executable();
    let project = project_with_plan(dir.path(), "MyProject", &argv_for(&executable, flag));
    let other = dir.path().join("OtherProject");
    std::fs::create_dir_all(&other).unwrap();
    let registry = dir.path().join("registry");
    register(
        &registry,
        "unity",
        18080,
        &[("dcc_mcp_project", &other.display().to_string())],
    );

    let value = start_instance(request(&project, &registry, 300))
        .await
        .unwrap();

    assert_eq!(value["reused"], false);
    let neighbours = value["diagnostics"]["other_project_instances"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(neighbours.len(), 1);
    assert_eq!(neighbours[0]["project"], other.display().to_string());
    // A new host is launched rather than adopting the other project's Editor.
    let operation = LifecycleStore::new(&registry)
        .find_owned("unity", &project)
        .unwrap()
        .unwrap();
    assert!(operation.launched);
}

// ── Blocking-state classification ──────────────────────────────────────────

#[tokio::test]
async fn advertised_blocking_state_drives_the_next_action() {
    let dir = tempfile::tempdir().unwrap();
    let (executable, flag) = noop_executable();
    let project = project_with_plan(dir.path(), "MyProject", &argv_for(&executable, flag));
    let registry = dir.path().join("registry");
    register(
        &registry,
        "unity",
        18080,
        &[
            ("dcc_mcp_project", &project.display().to_string()),
            ("license_state", "expired"),
        ],
    );

    let value = start_instance(request(&project, &registry, 300))
        .await
        .unwrap();

    assert_eq!(value["reused"], true);
    assert_eq!(value["ok"], false);
    assert_eq!(value["blocking_state"], "license");
    assert_eq!(value["next_action"]["id"], "resolve_license");
    assert_eq!(value["retryable"], false);
}

#[test]
fn registration_prefers_operation_id_over_pid() {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let by_operation = register(
        &registry,
        "unity",
        18080,
        &[("dcc_mcp_operation_id", "op-1")],
    );
    let _by_pid = register(
        &registry,
        "unity",
        18081,
        &[("dcc_mcp_operation_id", "op-2")],
    );

    let entries = local_instance::select_entries(&registry, Some("unity"), None).unwrap();
    let selected = select_registration(&entries, dir.path(), "op-1", Some(4242)).unwrap();

    assert_eq!(selected.instance_id, by_operation.instance_id);
}

#[test]
fn registration_falls_back_to_the_launched_pid() {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let file_registry = registry_handle(&registry);
    let mut launched = ServiceEntry::new("unity", "127.0.0.1", 18080);
    // The launch has no sidecar process of its own here, so the row advertises
    // the host pid only. It must be a live pid: the registry prunes rows whose
    // bound host has exited.
    let host_pid = std::process::id();
    launched.pid = None;
    launched.host_pid = Some(host_pid);
    file_registry.register(launched.clone()).unwrap();

    let entries = local_instance::select_entries(&registry, Some("unity"), None).unwrap();
    let selected = select_registration(&entries, dir.path(), "op-unknown", Some(host_pid)).unwrap();

    assert_eq!(selected.instance_id, launched.instance_id);
}

#[test]
fn unbound_and_other_project_neighbours_are_reported_separately() {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let project = dir.path().join("MyProject");
    std::fs::create_dir_all(&project).unwrap();
    let other = dir.path().join("Other");
    std::fs::create_dir_all(&other).unwrap();
    register(&registry, "unity", 18080, &[]);
    register(
        &registry,
        "unity",
        18081,
        &[("dcc_mcp_project", &other.display().to_string())],
    );

    let entries = local_instance::select_entries(&registry, Some("unity"), None).unwrap();
    let (unbound, others) = classify_neighbours(&entries, &project);

    assert_eq!(unbound.len(), 1);
    assert_eq!(others.len(), 1);
    assert_eq!(others[0]["project"], other.display().to_string());
}

// ── Guarded stop ───────────────────────────────────────────────────────────

#[test]
fn owned_operation_guard_rejects_unowned_and_mismatched_targets() {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let store = LifecycleStore::new(&registry);
    let project = dir.path().join("MyProject");

    let mut operation = LifecycleOperation::new(
        "op-owned",
        "unity",
        &project,
        std::path::Path::new("/opt/unity/Editor/Unity"),
        None,
    );
    operation.launched = true;
    operation.owned = true;
    operation.pid = Some(4242);
    operation.instance_id = Some("11111111-2222-3333-4444-555555555555".to_string());
    store.save(&operation).unwrap();

    let mut adopted = LifecycleOperation::new(
        "op-adopted",
        "unity",
        &project,
        std::path::Path::new("/opt/unity/Editor/Unity"),
        None,
    );
    adopted.launched = false;
    adopted.owned = false;
    store.save(&adopted).unwrap();

    let owned = resolve_owned_operation(&registry, Some("unity"), None, Some("op-owned"))
        .unwrap()
        .unwrap();
    assert_eq!(owned.operation_id, "op-owned");

    assert!(resolve_owned_operation(&registry, Some("unity"), None, Some("op-adopted")).is_err());
    assert!(resolve_owned_operation(&registry, Some("maya"), None, Some("op-owned")).is_err());
    assert!(
        resolve_owned_operation(
            &registry,
            Some("unity"),
            Some("99999999-2222-3333-4444-555555555555"),
            Some("op-owned")
        )
        .is_err()
    );
    assert!(
        resolve_owned_operation(&registry, None, None, None)
            .unwrap()
            .is_none()
    );
}

#[test]
fn operation_summary_is_redacted_and_stable() {
    let operation = LifecycleOperation::new(
        "op-1",
        "unity",
        std::path::Path::new("/work/MyProject"),
        std::path::Path::new("/opt/unity/Editor/Unity"),
        Some("2022.3.10f1".to_string()),
    );

    let summary = operation_summary(&operation);

    assert_eq!(summary["operation_id"], "op-1");
    assert_eq!(summary["dcc_type"], "unity");
    assert_eq!(summary["owned"], false);
    assert!(summary.get("argv").is_none());
}

// ── Plan resolution ordering ───────────────────────────────────────────────

#[tokio::test]
async fn explicit_launch_plan_wins_over_the_project_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let (executable, flag) = noop_executable();
    let project = project_with_plan(dir.path(), "MyProject", &argv_for(&executable, flag));
    let registry = dir.path().join("registry");

    let explicit = dir.path().join("explicit-plan.json");
    let explicit_executable = dir.path().join("ExplicitUnity");
    std::fs::write(&explicit_executable, b"x").unwrap();
    std::fs::write(
        &explicit,
        serde_json::json!({
            "schema_version": 1,
            "dcc_type": "unity",
            "executable": explicit_executable.display().to_string(),
            "argv": [PLACEHOLDER_EXECUTABLE, "-projectPath", PLACEHOLDER_PROJECT],
            "version": "6000.0.1f1",
        })
        .to_string(),
    )
    .unwrap();

    let mut receipt_request = request(&project, &registry, 200);
    receipt_request.dry_run = true;
    let value = start_instance(receipt_request).await.unwrap();
    assert_eq!(value["launch_plan"]["source"], "project_receipt");

    let mut explicit_request = request(&project, &registry, 200);
    explicit_request.dry_run = true;
    explicit_request.launch_plan = Some(explicit);
    let value = start_instance(explicit_request).await.unwrap();

    assert_eq!(value["launch_plan"]["source"], "explicit");
    assert_eq!(value["launch_plan"]["version"], "6000.0.1f1");
}

#[tokio::test]
async fn state_registry_plan_is_used_when_the_project_has_no_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("MyProject");
    std::fs::create_dir_all(&project).unwrap();
    let registry = dir.path().join("registry");
    std::fs::create_dir_all(registry.join(LAUNCH_PLAN_DIR_NAME)).unwrap();
    let executable = dir.path().join("StateUnity");
    std::fs::write(&executable, b"x").unwrap();
    std::fs::write(
        registry.join(LAUNCH_PLAN_DIR_NAME).join("unity.json"),
        serde_json::json!({
            "schema_version": 1,
            "dcc_type": "unity",
            "executable": executable.display().to_string(),
            "argv": [PLACEHOLDER_EXECUTABLE, "-projectPath", PLACEHOLDER_PROJECT],
        })
        .to_string(),
    )
    .unwrap();

    let mut request = request(&project, &registry, 200);
    request.dry_run = true;
    let value = start_instance(request).await.unwrap();

    assert_eq!(value["launch_plan"]["source"], "state_registry");
    assert_eq!(value["launch_plan_source"], serde_json::Value::Null);
}

// ── Metadata helpers ───────────────────────────────────────────────────────

#[test]
fn entry_metadata_prefers_the_namespaced_key() {
    let mut entry = ServiceEntry::new("unity", "127.0.0.1", 18080);
    entry
        .metadata
        .insert("project".to_string(), "  ".to_string());
    entry
        .metadata
        .insert("dcc_mcp_project".to_string(), "/work/MyProject".to_string());
    let mut extras: HashMap<String, Value> = HashMap::new();
    extras.insert("project".to_string(), Value::String("/extras".to_string()));
    entry.extras = extras;

    assert_eq!(
        entry_metadata(&entry, PROJECT_METADATA_KEYS),
        Some("/work/MyProject")
    );
    assert_eq!(
        entry_metadata(&entry, &["missing", "project"]),
        Some("/extras")
    );
}

#[test]
fn same_path_compares_canonicalized_paths() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("MyProject");
    std::fs::create_dir_all(&project).unwrap();
    let canonical = std::fs::canonicalize(&project).unwrap();

    assert!(same_path(&project, &canonical));
    assert!(!same_path(&project, &dir.path().join("Other")));
}
