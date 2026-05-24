use super::*;
use std::sync::Mutex;

/// In-memory test fake. Lets us drive the service without spinning
/// up a real SkillCatalog/ToolDispatcher — keeps unit tests
/// dependency-free.
#[derive(Default)]
struct FakeCatalog {
    actions: Mutex<Vec<CatalogAction>>,
}

impl FakeCatalog {
    fn push(&self, a: CatalogAction) {
        self.actions.lock().unwrap().push(a);
    }
}

impl SkillCatalogSource for FakeCatalog {
    fn list_actions(&self) -> Vec<CatalogAction> {
        self.actions.lock().unwrap().clone()
    }
    fn is_loaded(&self, name: &str) -> bool {
        self.actions
            .lock()
            .unwrap()
            .iter()
            .any(|a| a.skill_name == name && a.loaded)
    }
    fn load_skill(&self, skill_name: &str) -> Result<Vec<String>, ServiceError> {
        let mut actions = self.actions.lock().unwrap();
        let mut loaded = Vec::new();
        for action in actions.iter_mut().filter(|a| a.skill_name == skill_name) {
            action.loaded = true;
            loaded.push(action.action_name.clone());
        }
        if loaded.is_empty() {
            Err(ServiceError::new(
                ServiceErrorKind::NotFound,
                format!("skill not found: {skill_name}"),
            ))
        } else {
            Ok(loaded)
        }
    }
    fn unload_skill(&self, skill_name: &str) -> Result<usize, ServiceError> {
        let mut actions = self.actions.lock().unwrap();
        let mut removed = 0usize;
        for action in actions.iter_mut().filter(|a| a.skill_name == skill_name) {
            action.loaded = false;
            removed += 1;
        }
        if removed == 0 {
            Err(ServiceError::new(
                ServiceErrorKind::NotFound,
                format!("skill not found: {skill_name}"),
            ))
        } else {
            Ok(removed)
        }
    }
}

#[derive(Default)]
struct FakeInvoker {
    calls: Mutex<Vec<(String, Value)>>,
    next: Mutex<Option<Result<Value, ServiceError>>>,
}

impl FakeInvoker {
    fn set_next(&self, r: Result<Value, ServiceError>) {
        *self.next.lock().unwrap() = Some(r);
    }
}

impl ToolInvoker for FakeInvoker {
    fn invoke(&self, name: &str, params: Value) -> Result<CallOutcome, ServiceError> {
        self.calls
            .lock()
            .unwrap()
            .push((name.to_owned(), params.clone()));
        let r = self.next.lock().unwrap().take().unwrap_or(Ok(Value::Null));
        r.map(|v| CallOutcome {
            slug: ToolSlug(name.to_owned()),
            output: v,
            validation_skipped: false,
        })
    }
}

fn sphere_action(loaded: bool) -> CatalogAction {
    CatalogAction {
        action_name: "create_sphere".into(),
        skill_name: "spheres".into(),
        dcc: "maya".into(),
        description: "Create a polygon sphere".into(),
        tags: vec!["geometry".into(), "poly".into()],
        input_schema: serde_json::json!({"type":"object"}),
        loaded,
        scope: "repo".into(),
    }
}

fn build_service(actions: Vec<CatalogAction>) -> (SkillRestService, Arc<FakeInvoker>) {
    let cat = Arc::new(FakeCatalog::default());
    for a in actions {
        cat.push(a);
    }
    let inv = Arc::new(FakeInvoker::default());
    let svc = SkillRestService::new(cat, inv.clone());
    (svc, inv)
}

#[test]
fn slug_round_trip() {
    let s = ToolSlug::build("maya", "spheres", "create_sphere");
    let (d, sk, a) = s.parts().unwrap();
    assert_eq!((d, sk, a), ("maya", "spheres", "create_sphere"));
}

#[test]
fn slug_rejects_empty_parts() {
    assert!(ToolSlug("maya..create".into()).parts().is_none());
    assert!(ToolSlug("maya.spheres".into()).parts().is_none());
    assert!(ToolSlug(".spheres.create".into()).parts().is_none());
}

#[test]
fn search_returns_loaded_only_by_default() {
    let (svc, _) = build_service(vec![
        sphere_action(true),
        CatalogAction {
            action_name: "create_cube".into(),
            skill_name: "cubes".into(),
            loaded: false,
            ..sphere_action(true)
        },
    ]);
    let resp = svc.search(&SearchRequest::default());
    assert_eq!(resp.total, 1);
    assert_eq!(resp.hits[0].action, "create_sphere");
}

#[test]
fn search_loaded_only_false_returns_executable_next_step() {
    let (svc, _) = build_service(vec![sphere_action(false)]);
    let resp = svc.search(&SearchRequest {
        loaded_only: false,
        ..Default::default()
    });

    assert_eq!(resp.total, 1);
    let next = resp.hits[0].next_step.as_ref().expect("next_step");
    assert_eq!(next.action, "load_skill");
    assert_eq!(next.arguments["skill_name"], "spheres");
    assert_eq!(next.arguments["dcc"], "maya");
}

#[test]
fn load_skill_then_search_makes_action_callable() {
    let (svc, _) = build_service(vec![sphere_action(false)]);

    let loaded = svc
        .load_skill(&LoadSkillRequest {
            skill_name: "spheres".into(),
        })
        .unwrap();
    assert_eq!(loaded.actions, vec!["create_sphere"]);

    let resp = svc.search(&SearchRequest::default());
    assert_eq!(resp.total, 1);
    assert!(resp.hits[0].loaded);
    assert!(resp.hits[0].next_step.is_none());
}

#[test]
fn search_query_matches_description() {
    let (svc, _) = build_service(vec![sphere_action(true)]);
    let req = SearchRequest {
        query: Some("polygon".into()),
        ..Default::default()
    };
    assert_eq!(svc.search(&req).total, 1);
    let req = SearchRequest {
        query: Some("quaternion".into()),
        ..Default::default()
    };
    assert_eq!(svc.search(&req).total, 0);
}

#[test]
fn search_dcc_filter_is_case_insensitive() {
    let (svc, _) = build_service(vec![sphere_action(true)]);
    let req = SearchRequest {
        dcc_type: Some("MAYA".into()),
        ..Default::default()
    };
    assert_eq!(svc.search(&req).total, 1);
}

#[test]
fn search_tags_are_anded() {
    let (svc, _) = build_service(vec![sphere_action(true)]);
    let req = SearchRequest {
        tags: vec!["geometry".into(), "poly".into()],
        ..Default::default()
    };
    assert_eq!(svc.search(&req).total, 1);
    let req = SearchRequest {
        tags: vec!["geometry".into(), "rig".into()],
        ..Default::default()
    };
    assert_eq!(svc.search(&req).total, 0);
}

#[test]
fn search_limit_caps_hits() {
    let mut many = Vec::new();
    for i in 0..5 {
        let mut a = sphere_action(true);
        a.action_name = format!("create_{i}");
        many.push(a);
    }
    let (svc, _) = build_service(many);
    let req = SearchRequest {
        limit: Some(2),
        ..Default::default()
    };
    assert_eq!(svc.search(&req).total, 2);
}

#[test]
fn describe_returns_schema_when_asked() {
    let (svc, _) = build_service(vec![sphere_action(true)]);
    let slug = ToolSlug::build("maya", "spheres", "create_sphere");
    let d = svc
        .describe(&DescribeRequest {
            tool_slug: slug.clone(),
            include_schema: true,
        })
        .unwrap();
    assert!(d.input_schema.is_some());
    let d = svc
        .describe(&DescribeRequest {
            tool_slug: slug,
            include_schema: false,
        })
        .unwrap();
    assert!(d.input_schema.is_none());
}

#[test]
fn catalog_source_lists_discovered_tools_with_input_schema() {
    use dcc_mcp_actions::ToolRegistry;
    use dcc_mcp_skills::SkillCatalog;
    use std::sync::Arc;

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    let skill_dir = repo_root.join("examples/skills/multi-script");
    if !skill_dir.is_dir() {
        return;
    }

    let registry = Arc::new(ToolRegistry::new());
    let catalog = Arc::new(SkillCatalog::new(registry));
    let path = skill_dir.to_string_lossy().into_owned();
    assert!(catalog.discover(Some(&[path]), Some("python")) > 0);
    assert!(!catalog.is_loaded("multi-script"));

    let catalog_src = Arc::new(CatalogSource::new(catalog.clone()));
    let actions = catalog_src.list_actions();
    let action = actions
        .iter()
        .find(|a| a.action_name == "multi_script__action_python")
        .expect("discovered tool should be indexed before load_skill");
    assert!(!action.loaded);
    assert!(
        action
            .input_schema
            .get("properties")
            .and_then(|p| p.get("message"))
            .is_some(),
        "tools.yaml properties must survive on discovered tools"
    );

    let svc = SkillRestService::new(catalog_src, Arc::new(FakeInvoker::default()));
    let slug = ToolSlug::build("python", "multi-script", "multi_script__action_python");
    let hit = svc
        .search(&SearchRequest {
            query: Some("action_python".into()),
            loaded_only: false,
            ..Default::default()
        })
        .hits
        .into_iter()
        .find(|h| h.action == "multi_script__action_python")
        .expect("search should surface discovered tool");
    assert!(hit.has_schema);

    let described = svc
        .describe(&DescribeRequest {
            tool_slug: slug,
            include_schema: true,
        })
        .unwrap();
    let schema = described.input_schema.expect("describe must return schema");
    assert!(
        schema
            .get("properties")
            .and_then(|p| p.get("message"))
            .is_some()
    );
}

#[test]
fn describe_unknown_slug_is_404_class() {
    let (svc, _) = build_service(vec![]);
    let err = svc
        .describe(&DescribeRequest {
            tool_slug: ToolSlug::build("maya", "missing", "tool"),
            include_schema: true,
        })
        .unwrap_err();
    assert_eq!(err.kind, ServiceErrorKind::UnknownSlug);
}

#[test]
fn call_rejects_unloaded_skill() {
    let (svc, _) = build_service(vec![sphere_action(false)]);
    let err = svc
        .call(&CallRequest {
            tool_slug: ToolSlug::build("maya", "spheres", "create_sphere"),
            params: Value::Null,
        })
        .unwrap_err();
    assert_eq!(err.kind, ServiceErrorKind::SkillNotLoaded);
}

#[test]
fn dispatch_veto_maps_to_policy_denied() {
    let err = dispatch_error_to_service_error(DispatchError::Vetoed {
        action: "delete_scene".into(),
        code: "policy_denied".into(),
        reason: "destructive tools are disabled".into(),
    });

    assert_eq!(err.kind, ServiceErrorKind::PolicyDenied);
    assert_eq!(err.http_status(), 403);
    assert_eq!(err.context.as_ref().unwrap()["veto_code"], "policy_denied");
}

#[test]
fn queue_overload_maps_to_host_busy() {
    for message in [
        "host-busy",
        "queue-overloaded: depth=16/16; retry in 1s",
        "queue overloaded (depth=16/16); retry in 1s",
    ] {
        let err = dispatch_error_to_service_error(DispatchError::HandlerError(message.into()));
        assert_eq!(err.kind, ServiceErrorKind::HostBusy);
        assert_eq!(err.http_status(), 503);
    }
}

#[test]
fn call_dispatches_and_normalises_slug() {
    let (svc, inv) = build_service(vec![sphere_action(true)]);
    inv.set_next(Ok(serde_json::json!({"created": 1})));
    let out = svc
        .call(&CallRequest {
            tool_slug: ToolSlug::build("maya", "spheres", "create_sphere"),
            params: serde_json::json!({"radius": 1.5}),
        })
        .unwrap();
    assert_eq!(out.slug.0, "maya.spheres.create_sphere");
    assert_eq!(out.output["created"], 1);
    let calls = inv.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "create_sphere");
    assert_eq!(calls[0].1["radius"], 1.5);
}

#[test]
fn invalid_slug_format_is_bad_request() {
    let (svc, _) = build_service(vec![sphere_action(true)]);
    let err = svc
        .call(&CallRequest {
            tool_slug: ToolSlug("not-a-slug".into()),
            params: Value::Null,
        })
        .unwrap_err();
    assert_eq!(err.kind, ServiceErrorKind::BadRequest);
}

#[test]
fn context_snapshot_counts_loaded_skills() {
    let (svc, _) = build_service(vec![
        sphere_action(true),
        CatalogAction {
            skill_name: "cubes".into(),
            loaded: true,
            ..sphere_action(true)
        },
        CatalogAction {
            skill_name: "ghosts".into(),
            loaded: false,
            ..sphere_action(false)
        },
    ]);
    svc.update_context(|c| c.dcc = Some("maya".into()));
    let snap = svc.context_snapshot();
    assert_eq!(snap.dcc.as_deref(), Some("maya"));
    assert_eq!(snap.action_count, 3);
    assert_eq!(snap.loaded_skill_count, 2);
}

/// Regression guard against token-budget bloat on /v1/search. A
/// single hit must fit inside a strict byte budget so agents can
/// page through hundreds of tools per turn without blowing the
/// context window.
#[test]
fn search_hit_stays_under_token_budget() {
    let mut long = sphere_action(true);
    long.description = "x".repeat(5000); // absurdly long on purpose
    let (svc, _) = build_service(vec![long]);
    let resp = svc.search(&SearchRequest::default());
    let hit = &resp.hits[0];
    let serialised = serde_json::to_string(hit).unwrap();
    assert!(
        serialised.len() < crate::SEARCH_HIT_BUDGET_BYTES,
        "search hit serialised to {} bytes (>{} budget) — probable schema expansion",
        serialised.len(),
        crate::SEARCH_HIT_BUDGET_BYTES,
    );
}
