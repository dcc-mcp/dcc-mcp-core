//! Launch-plan resolution for `dcc-mcp-cli start-instance`.
//!
//! Core consumes adapter-authored launch plans and never synthesizes a
//! DCC-specific command line. Resolution is deterministic and most specific
//! source wins: explicit `--launch-plan` → project receipt → core state registry.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::start_instance::{
    LAUNCH_PLAN_DIR_NAME, LaunchPlan, LaunchPlanDocument, LaunchPlanError, LaunchPlanSource,
    PROJECT_LAUNCH_PLAN_RELATIVE_PATH, StartInstanceRequest,
};

/// Outcome of resolving a launch plan for one DCC type + project.
#[derive(Debug)]
pub enum PlanResolution {
    Resolved(LaunchPlan),
    /// No candidate plan exists. Callers report
    /// `BlockingState::LaunchPlanMissing`.
    Missing,
    /// A candidate exists but is unreadable, malformed, or unsafe.
    Invalid {
        plan_path: Option<PathBuf>,
        error: LaunchPlanError,
    },
}

/// Resolve the launch plan for `request`, bound to the canonical `project`.
///
/// `start-instance` never downloads or installs anything, so a missing plan is a
/// terminal condition rather than a trigger for remediation.
pub fn resolve(request: &StartInstanceRequest, project: &Path) -> PlanResolution {
    if let Some(path) = request.launch_plan.as_deref() {
        return resolve_document_at(
            path,
            project,
            &request.dcc_type,
            request.version.as_deref(),
            LaunchPlanSource::Explicit,
        );
    }

    let receipt = project.join(PROJECT_LAUNCH_PLAN_RELATIVE_PATH);
    if receipt.is_file() {
        return resolve_document_at(
            &receipt,
            project,
            &request.dcc_type,
            request.version.as_deref(),
            LaunchPlanSource::ProjectReceipt,
        );
    }

    let state_plan = request
        .registry_dir
        .join(LAUNCH_PLAN_DIR_NAME)
        .join(format!("{}.json", normalized_dcc_key(&request.dcc_type)));
    if state_plan.is_file() {
        return resolve_document_at(
            &state_plan,
            project,
            &request.dcc_type,
            request.version.as_deref(),
            LaunchPlanSource::StateRegistry,
        );
    }

    PlanResolution::Missing
}

/// True when a validated launch plan exists for `dcc_type` + `project`.
///
/// Used by discovery so the zero-instance next action can be `start_instance`
/// instead of a blind install. Never launches anything.
pub fn resolvable(dcc_type: &str, project: &Path, registry_dir: &Path) -> bool {
    let request = StartInstanceRequest {
        dcc_type: dcc_type.to_string(),
        project: project.to_path_buf(),
        launch_plan: None,
        version: None,
        wait_ready: false,
        timeout: Duration::from_secs(0),
        interval: Duration::from_secs(1),
        required: Vec::new(),
        authorized: false,
        dry_run: true,
        registry_dir: registry_dir.to_path_buf(),
        instance_id: None,
    };
    matches!(resolve(&request, project), PlanResolution::Resolved(_))
}

/// Paths a caller can inspect when no plan resolved, for diagnostics only.
pub fn candidate_paths(request: &StartInstanceRequest, project: &Path) -> Vec<PathBuf> {
    vec![
        project.join(PROJECT_LAUNCH_PLAN_RELATIVE_PATH),
        request
            .registry_dir
            .join(LAUNCH_PLAN_DIR_NAME)
            .join(format!("{}.json", normalized_dcc_key(&request.dcc_type))),
    ]
}

fn resolve_document_at(
    path: &Path,
    project: &Path,
    dcc_type: &str,
    version: Option<&str>,
    source: LaunchPlanSource,
) -> PlanResolution {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => {
            return PlanResolution::Invalid {
                plan_path: Some(path.to_path_buf()),
                error: LaunchPlanError::UnreadablePlan(path.to_path_buf()),
            };
        }
    };
    let document: LaunchPlanDocument = match serde_json::from_str(&raw) {
        Ok(document) => document,
        Err(error) => {
            return PlanResolution::Invalid {
                plan_path: Some(path.to_path_buf()),
                error: LaunchPlanError::MalformedPlan(error.to_string()),
            };
        }
    };
    match LaunchPlan::from_document(
        document,
        dcc_type,
        project,
        version,
        source,
        Some(path.to_path_buf()),
    ) {
        Ok(plan) => PlanResolution::Resolved(plan),
        Err(error) => PlanResolution::Invalid {
            plan_path: Some(path.to_path_buf()),
            error,
        },
    }
}

/// First project-binding marker declared by the plan that is absent on disk.
///
/// Markers let Core detect a project that is present on disk but is not the
/// project the adapter was installed for, instead of launching the wrong host.
pub fn missing_project_marker(plan: &LaunchPlan, project: &Path) -> Option<String> {
    plan.project_markers
        .iter()
        .map(|marker| marker.trim().trim_start_matches(['/', '\\']))
        .filter(|marker| !marker.is_empty())
        .find(|marker| !project.join(marker).exists())
        .map(str::to_string)
}

pub fn normalized_dcc_key(dcc_type: &str) -> String {
    dcc_type
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::start_instance::{PLACEHOLDER_EXECUTABLE, PLACEHOLDER_PROJECT};

    fn request(project: &Path, registry: &Path) -> StartInstanceRequest {
        StartInstanceRequest {
            dcc_type: "unity".to_string(),
            project: project.to_path_buf(),
            launch_plan: None,
            version: None,
            wait_ready: true,
            timeout: Duration::from_secs(30),
            interval: Duration::from_secs(1),
            required: Vec::new(),
            authorized: true,
            dry_run: false,
            registry_dir: registry.to_path_buf(),
            instance_id: None,
        }
    }

    fn write_plan(_dir: &Path, dcc_type: &str, executable: &Path) -> String {
        serde_json::json!({
            "schema_version": 1,
            "dcc_type": dcc_type,
            "executable": executable.display().to_string(),
            "argv": [PLACEHOLDER_EXECUTABLE, "-projectPath", PLACEHOLDER_PROJECT],
            "version": "2022.3.10f1",
            "project_markers": ["ProjectSettings/ProjectVersion.txt"],
        })
        .to_string()
    }

    #[test]
    fn project_receipt_wins_over_state_registry() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("MyProject");
        std::fs::create_dir_all(project.join(".dcc-mcp")).unwrap();
        std::fs::create_dir_all(project.join("ProjectSettings")).unwrap();
        std::fs::write(project.join("ProjectSettings/ProjectVersion.txt"), b"x").unwrap();
        let receipt_exec = dir.path().join("ReceiptUnity");
        std::fs::write(&receipt_exec, b"x").unwrap();
        std::fs::write(
            project.join(PROJECT_LAUNCH_PLAN_RELATIVE_PATH),
            write_plan(dir.path(), "unity", &receipt_exec),
        )
        .unwrap();

        let registry = dir.path().join("registry");
        let state_exec = dir.path().join("StateUnity");
        std::fs::write(&state_exec, b"x").unwrap();
        std::fs::create_dir_all(registry.join(LAUNCH_PLAN_DIR_NAME)).unwrap();
        std::fs::write(
            registry.join(LAUNCH_PLAN_DIR_NAME).join("unity.json"),
            write_plan(dir.path(), "unity", &state_exec),
        )
        .unwrap();

        let resolved = resolve(&request(&project, &registry), &project);
        let PlanResolution::Resolved(plan) = resolved else {
            panic!("expected a resolved launch plan");
        };

        assert_eq!(plan.source, LaunchPlanSource::ProjectReceipt);
        assert_eq!(plan.executable, receipt_exec);
    }

    #[test]
    fn state_registry_plan_is_used_when_the_project_has_no_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("MyProject");
        std::fs::create_dir_all(&project).unwrap();
        let registry = dir.path().join("registry");
        let state_exec = dir.path().join("StateUnity");
        std::fs::write(&state_exec, b"x").unwrap();
        std::fs::create_dir_all(registry.join(LAUNCH_PLAN_DIR_NAME)).unwrap();
        std::fs::write(
            registry.join(LAUNCH_PLAN_DIR_NAME).join("unity.json"),
            write_plan(dir.path(), "unity", &state_exec),
        )
        .unwrap();

        let resolved = resolve(&request(&project, &registry), &project);
        let PlanResolution::Resolved(plan) = resolved else {
            panic!("expected a resolved launch plan");
        };

        assert_eq!(plan.source, LaunchPlanSource::StateRegistry);
    }

    #[test]
    fn no_plan_resolves_to_missing() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("MyProject");
        std::fs::create_dir_all(&project).unwrap();
        let registry = dir.path().join("registry");

        assert!(matches!(
            resolve(&request(&project, &registry), &project),
            PlanResolution::Missing
        ));
    }

    #[test]
    fn malformed_plan_is_invalid_not_missing() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("MyProject");
        std::fs::create_dir_all(project.join(".dcc-mcp")).unwrap();
        std::fs::write(
            project.join(PROJECT_LAUNCH_PLAN_RELATIVE_PATH),
            b"{not json",
        )
        .unwrap();
        let registry = dir.path().join("registry");

        let resolved = resolve(&request(&project, &registry), &project);
        let PlanResolution::Invalid { error, .. } = resolved else {
            panic!("expected an invalid launch plan");
        };
        assert!(matches!(error, LaunchPlanError::MalformedPlan(_)));
    }

    #[test]
    fn missing_markers_are_detected() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("MyProject");
        std::fs::create_dir_all(&project).unwrap();
        let plan = LaunchPlan {
            dcc_type: "unity".to_string(),
            executable: dir.path().join("Unity"),
            argv: vec!["Unity".to_string()],
            version: None,
            project_markers: vec!["ProjectSettings/ProjectVersion.txt".to_string()],
            cwd: None,
            source: LaunchPlanSource::Explicit,
            plan_path: None,
        };

        assert_eq!(
            missing_project_marker(&plan, &project),
            Some("ProjectSettings/ProjectVersion.txt".to_string())
        );

        std::fs::create_dir_all(project.join("ProjectSettings")).unwrap();
        std::fs::write(project.join("ProjectSettings/ProjectVersion.txt"), b"x").unwrap();
        assert_eq!(missing_project_marker(&plan, &project), None);
    }
}
