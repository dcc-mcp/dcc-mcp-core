use super::*;
use crate::callers::ToolCaller;
use crate::callers::test_support::{MockRemoteCaller, MockToolCaller};
use crate::notifier::RecordingNotifier;
use crate::policy::{RetryPolicy as PolicyRetryPolicy, StepPolicy as PolicyStepPolicy};
use crate::spec::{Step, StepId, StepKind, WorkflowSpec};
use serde_json::json;
use std::sync::Arc;

fn spec_with_steps(steps: Vec<Step>) -> WorkflowSpec {
    WorkflowSpec {
        name: "t".to_string(),
        description: String::new(),
        inputs: Value::Null,
        steps,
    }
}

fn tool_step(id: &str, tool: &str, args: Value) -> Step {
    Step {
        id: StepId(id.to_string()),
        kind: StepKind::Tool {
            tool: tool.to_string(),
            args,
        },
        policy: PolicyStepPolicy::default(),
    }
}

fn remote_step(id: &str, dcc: &str, tool: &str, args: Value) -> Step {
    Step {
        id: StepId(id.to_string()),
        kind: StepKind::ToolRemote {
            dcc: dcc.to_string(),
            tool: tool.to_string(),
            args,
        },
        policy: PolicyStepPolicy::default(),
    }
}

#[tokio::test]
async fn tool_step_runs_and_completes() {
    let mock = Arc::new(MockToolCaller::new());
    mock.add("echo", |args| Ok(json!({"echoed": args})));
    let rec = Arc::new(RecordingNotifier::new());

    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .notifier(rec.clone())
        .build();

    let spec = spec_with_steps(vec![tool_step("s1", "echo", json!({"x": 1}))]);
    let handle = exe.run(spec, Value::Null, None).unwrap();
    let status = handle.wait().await;
    assert_eq!(status, WorkflowStatus::Completed);
    assert_eq!(mock.call_count("echo"), 1);
    assert!(
        rec.len() >= 3,
        "expected enter/exit/terminal events, got {}",
        rec.len()
    );
}

#[tokio::test]
async fn tool_step_args_are_rendered_against_inputs() {
    let mock = Arc::new(MockToolCaller::new());
    let seen = Arc::new(parking_lot::Mutex::new(Value::Null));
    let seen_c = seen.clone();
    mock.add("echo", move |args| {
        *seen_c.lock() = args.clone();
        Ok(json!({"ok": true}))
    });
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let spec = spec_with_steps(vec![tool_step(
        "s1",
        "echo",
        json!({"name": "{{inputs.who}}"}),
    )]);
    let h = exe.run(spec, json!({"who": "alice"}), None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Completed);
    assert_eq!(*seen.lock(), json!({"name": "alice"}));
}

#[tokio::test]
async fn step_output_is_accessible_to_next_step() {
    let mock = Arc::new(MockToolCaller::new());
    mock.add("produce", |_| Ok(json!({"value": 42})));
    let seen = Arc::new(parking_lot::Mutex::new(Value::Null));
    let seen_c = seen.clone();
    mock.add("consume", move |args| {
        *seen_c.lock() = args.clone();
        Ok(Value::Null)
    });
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let spec = spec_with_steps(vec![
        tool_step("a", "produce", Value::Null),
        tool_step("b", "consume", json!({"v": "{{steps.a.output.value}}"})),
    ]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Completed);
    assert_eq!(*seen.lock(), json!({"v": 42}));
}

#[tokio::test]
async fn tool_step_failure_fails_workflow() {
    let mock = Arc::new(MockToolCaller::new());
    mock.add("boom", |_| Err("nope".to_string()));
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let spec = spec_with_steps(vec![tool_step("s", "boom", Value::Null)]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Failed);
}

#[tokio::test]
async fn retry_policy_retries_on_transient() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let attempts = Arc::new(AtomicU32::new(0));
    let a_c = attempts.clone();
    let mock = Arc::new(MockToolCaller::new());
    mock.add("flaky", move |_| {
        let n = a_c.fetch_add(1, Ordering::SeqCst);
        if n < 2 {
            Err("transient".to_string())
        } else {
            Ok(json!({"ok": true}))
        }
    });
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let mut step = tool_step("s", "flaky", Value::Null);
    step.policy.retry = Some(PolicyRetryPolicy {
        max_attempts: 5,
        backoff: BackoffKind::Fixed,
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(10),
        jitter: 0.0,
        retry_on: Some(vec!["transient".to_string()]),
    });
    let spec = spec_with_steps(vec![step]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Completed);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn retry_policy_stops_on_non_retryable() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let attempts = Arc::new(AtomicU32::new(0));
    let a_c = attempts.clone();
    let mock = Arc::new(MockToolCaller::new());
    mock.add("flaky", move |_| {
        a_c.fetch_add(1, Ordering::SeqCst);
        Err("validation".to_string())
    });
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let mut step = tool_step("s", "flaky", Value::Null);
    step.policy.retry = Some(PolicyRetryPolicy {
        max_attempts: 5,
        backoff: BackoffKind::Fixed,
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(10),
        jitter: 0.0,
        retry_on: Some(vec!["transient".to_string()]),
    });
    let spec = spec_with_steps(vec![step]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Failed);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn timeout_policy_fires() {
    let mock = Arc::new(MockToolCaller::new());
    // Handler completes instantly but sleeps inside tokio task.
    struct SlowCaller;
    impl ToolCaller for SlowCaller {
        fn call<'a>(
            &'a self,
            _n: &'a str,
            _a: Value,
            cancel: CancellationToken,
        ) -> crate::callers::CallFuture<'a> {
            Box::pin(async move {
                tokio::select! {
                    _ = cancel.cancelled() => Err("cancelled".to_string()),
                    _ = tokio::time::sleep(Duration::from_millis(500)) => Ok(Value::Null),
                }
            })
        }
    }
    let _ = mock;
    let exe = WorkflowExecutor::builder()
        .tool_caller(Arc::new(SlowCaller))
        .build();
    let mut step = tool_step("s", "slow", Value::Null);
    step.policy.timeout = Some(Duration::from_millis(20));
    let spec = spec_with_steps(vec![step]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Failed);
}

#[tokio::test]
async fn idempotency_key_short_circuits_second_call() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let mock = Arc::new(MockToolCaller::new());
    mock.add("op", move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"n": 1}))
    });
    let cache = IdempotencyCache::new();
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .idempotency(cache.clone())
        .build();
    let mut step1 = tool_step("s", "op", Value::Null);
    step1.policy.idempotency_key = Some("fixed-key".to_string());
    step1.policy.idempotency_scope = IdempotencyScope::Global;
    let spec1 = spec_with_steps(vec![step1.clone()]);
    let h1 = exe.run(spec1, Value::Null, None).unwrap();
    assert_eq!(h1.wait().await, WorkflowStatus::Completed);
    // Second workflow, same key, global scope → cached.
    let spec2 = spec_with_steps(vec![step1]);
    let h2 = exe.run(spec2, Value::Null, None).unwrap();
    assert_eq!(h2.wait().await, WorkflowStatus::Completed);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_aborts_workflow() {
    struct BlockingCaller;
    impl ToolCaller for BlockingCaller {
        fn call<'a>(
            &'a self,
            _n: &'a str,
            _a: Value,
            cancel: CancellationToken,
        ) -> crate::callers::CallFuture<'a> {
            Box::pin(async move {
                cancel.cancelled().await;
                Err("cancelled".to_string())
            })
        }
    }
    let exe = WorkflowExecutor::builder()
        .tool_caller(Arc::new(BlockingCaller))
        .build();
    let spec = spec_with_steps(vec![tool_step("s", "block", Value::Null)]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    let cancel = h.cancel_token.clone();
    let join = h.join;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
    });
    let status = tokio::time::timeout(Duration::from_millis(500), join)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, WorkflowStatus::Cancelled);
}

#[tokio::test]
async fn foreach_iterates_over_jsonpath_items() {
    let mock = Arc::new(MockToolCaller::new());
    mock.add("per", |args| Ok(json!({"got": args})));
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let foreach = Step {
        id: StepId("loop".into()),
        kind: StepKind::Foreach {
            items: "$.inputs.items".to_string(),
            r#as: "item".to_string(),
            steps: vec![tool_step("inner", "per", json!({"v": "{{item}}"}))],
        },
        policy: PolicyStepPolicy::default(),
    };
    let spec = spec_with_steps(vec![foreach]);
    let h = exe
        .run(spec, json!({"items": ["a", "b", "c"]}), None)
        .unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Completed);
    assert_eq!(mock.call_count("per"), 3);
}

#[tokio::test]
async fn parallel_runs_branches_concurrently() {
    let mock = Arc::new(MockToolCaller::new());
    mock.add("a", |_| Ok(json!({"from": "a"})));
    mock.add("b", |_| Ok(json!({"from": "b"})));
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let parallel = Step {
        id: StepId("par".into()),
        kind: StepKind::Parallel {
            steps: vec![
                tool_step("x", "a", Value::Null),
                tool_step("y", "b", Value::Null),
            ],
        },
        policy: PolicyStepPolicy::default(),
    };
    let spec = spec_with_steps(vec![parallel]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Completed);
    assert_eq!(mock.call_count("a"), 1);
    assert_eq!(mock.call_count("b"), 1);
}

#[tokio::test]
async fn parallel_any_failure_fails_workflow() {
    let mock = Arc::new(MockToolCaller::new());
    mock.add("ok", |_| Ok(Value::Null));
    mock.add("bad", |_| Err("fail".to_string()));
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let parallel = Step {
        id: StepId("par".into()),
        kind: StepKind::Parallel {
            steps: vec![
                tool_step("x", "ok", Value::Null),
                tool_step("y", "bad", Value::Null),
            ],
        },
        policy: PolicyStepPolicy::default(),
    };
    let spec = spec_with_steps(vec![parallel]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Failed);
}

#[tokio::test]
async fn branch_takes_then_on_truthy() {
    let mock = Arc::new(MockToolCaller::new());
    mock.add("then_path", |_| Ok(json!({"branch": "then"})));
    mock.add("else_path", |_| Ok(json!({"branch": "else"})));
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let branch = Step {
        id: StepId("gate".into()),
        kind: StepKind::Branch {
            on: "$.inputs.flag".to_string(),
            then: vec![tool_step("t", "then_path", Value::Null)],
            else_steps: vec![tool_step("e", "else_path", Value::Null)],
        },
        policy: PolicyStepPolicy::default(),
    };
    let spec = spec_with_steps(vec![branch]);
    let h = exe.run(spec, json!({"flag": true}), None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Completed);
    assert_eq!(mock.call_count("then_path"), 1);
    assert_eq!(mock.call_count("else_path"), 0);
}

#[tokio::test]
async fn branch_takes_else_on_falsy() {
    let mock = Arc::new(MockToolCaller::new());
    mock.add("then_path", |_| Ok(Value::Null));
    mock.add("else_path", |_| Ok(Value::Null));
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let branch = Step {
        id: StepId("gate".into()),
        kind: StepKind::Branch {
            on: "$.inputs.flag".to_string(),
            then: vec![tool_step("t", "then_path", Value::Null)],
            else_steps: vec![tool_step("e", "else_path", Value::Null)],
        },
        policy: PolicyStepPolicy::default(),
    };
    let spec = spec_with_steps(vec![branch]);
    let h = exe.run(spec, json!({"flag": false}), None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Completed);
    assert_eq!(mock.call_count("then_path"), 0);
    assert_eq!(mock.call_count("else_path"), 1);
}

#[tokio::test]
async fn approve_step_resolves_when_gate_approves() {
    let mock = Arc::new(MockToolCaller::new());
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let step = Step {
        id: StepId("ok_to_go".into()),
        kind: StepKind::Approve {
            prompt: "go?".to_string(),
        },
        policy: PolicyStepPolicy::default(),
    };
    let spec = spec_with_steps(vec![step]);
    let gate = exe.approval_gate();
    let h = exe.run(spec, Value::Null, None).unwrap();
    let wid = h.workflow_id;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        gate.resolve(
            wid,
            "ok_to_go",
            crate::approval::ApprovalResponse {
                approved: true,
                reason: None,
            },
        );
    });
    let status = tokio::time::timeout(Duration::from_secs(2), h.join)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, WorkflowStatus::Completed);
}

#[tokio::test]
async fn approve_step_times_out_when_policy_timeout_set() {
    let mock = Arc::new(MockToolCaller::new());
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .build();
    let mut step = Step {
        id: StepId("wait".into()),
        kind: StepKind::Approve {
            prompt: "go?".to_string(),
        },
        policy: PolicyStepPolicy::default(),
    };
    step.policy.timeout = Some(Duration::from_millis(40));
    let spec = spec_with_steps(vec![step]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    let status = tokio::time::timeout(Duration::from_secs(2), h.join)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, WorkflowStatus::Failed);
}

#[tokio::test]
async fn remote_step_invokes_remote_caller() {
    let local = Arc::new(MockToolCaller::new());
    let remote = Arc::new(MockRemoteCaller::new());
    remote.add("unreal", "ingest", |_| Ok(json!({"ok": true})));
    let exe = WorkflowExecutor::builder()
        .tool_caller(local.clone())
        .remote_caller(remote.clone())
        .build();
    let spec = spec_with_steps(vec![remote_step("r", "unreal", "ingest", Value::Null)]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Completed);
    assert_eq!(remote.calls.lock().len(), 1);
}

#[tokio::test]
async fn workflow_emits_terminal_notifier_event() {
    let mock = Arc::new(MockToolCaller::new());
    mock.add("echo", |_| Ok(Value::Null));
    let rec = Arc::new(RecordingNotifier::new());
    let exe = WorkflowExecutor::builder()
        .tool_caller(mock.clone())
        .notifier(rec.clone())
        .build();
    let spec = spec_with_steps(vec![tool_step("s", "echo", Value::Null)]);
    let h = exe.run(spec, Value::Null, None).unwrap();
    assert_eq!(h.wait().await, WorkflowStatus::Completed);
    let events = rec.events();
    assert!(matches!(
        events.last().unwrap().status,
        WorkflowStatus::Completed
    ));
}

#[test]
fn count_steps_counts_nested() {
    let spec = spec_with_steps(vec![Step {
        id: StepId("p".into()),
        kind: StepKind::Parallel {
            steps: vec![
                tool_step("a", "x", Value::Null),
                tool_step("b", "y", Value::Null),
            ],
        },
        policy: PolicyStepPolicy::default(),
    }]);
    assert_eq!(count_steps(&spec), 3);
}

#[test]
fn is_truthy_sanity() {
    assert!(!is_truthy(&Value::Null));
    assert!(!is_truthy(&json!(false)));
    assert!(!is_truthy(&json!(0)));
    assert!(!is_truthy(&json!("")));
    assert!(!is_truthy(&json!([])));
    assert!(!is_truthy(&json!({})));
    assert!(is_truthy(&json!(true)));
    assert!(is_truthy(&json!(1)));
    assert!(is_truthy(&json!("x")));
    assert!(is_truthy(&json!([1])));
    assert!(is_truthy(&json!({"a": 1})));
}

#[cfg(feature = "job-persist-sqlite")]
#[tokio::test]
async fn idempotency_persists_across_executor_rebuild_via_sqlite() {
    // Locks in the issue #566 acceptance criterion: a workflow with an
    // idempotency key writes through to SQLite, and a *different*
    // executor instance built against the same on-disk DB short-circuits
    // the next call. This is the round-trip the in-memory cache could
    // never deliver and is the foundation #565 (workflows.resume) builds
    // on for skip-on-replay semantics.
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::sqlite::{SqliteIdempotencyStore, WorkflowStorage};

    let db = tempfile::NamedTempFile::new().unwrap();
    let storage_a = Arc::new(WorkflowStorage::open(db.path()).unwrap());

    let calls = Arc::new(AtomicU32::new(0));
    let calls_c = calls.clone();
    let mock_a = Arc::new(MockToolCaller::new());
    mock_a.add("op", move |_| {
        calls_c.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"n": 1}))
    });

    let exe_a = WorkflowExecutor::builder()
        .tool_caller(mock_a.clone())
        .idempotency_store(SqliteIdempotencyStore::new(Arc::clone(&storage_a)))
        .storage(Arc::clone(&storage_a))
        .build();
    let mut step = tool_step("s", "op", Value::Null);
    step.policy.idempotency_key = Some("export-fixed".to_string());
    step.policy.idempotency_scope = IdempotencyScope::Global;
    let spec1 = spec_with_steps(vec![step.clone()]);
    let h1 = exe_a.run(spec1, Value::Null, None).unwrap();
    assert_eq!(h1.wait().await, WorkflowStatus::Completed);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(exe_a);
    drop(storage_a);

    // Rebuild executor + storage from scratch over the same DB file —
    // simulating a server restart.
    let storage_b = Arc::new(WorkflowStorage::open(db.path()).unwrap());
    let mock_b = Arc::new(MockToolCaller::new());
    let calls_c2 = calls.clone();
    mock_b.add("op", move |_| {
        calls_c2.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"n": 1}))
    });
    let exe_b = WorkflowExecutor::builder()
        .tool_caller(mock_b.clone())
        .idempotency_store(SqliteIdempotencyStore::new(Arc::clone(&storage_b)))
        .storage(storage_b)
        .build();
    let spec2 = spec_with_steps(vec![step]);
    let h2 = exe_b.run(spec2, Value::Null, None).unwrap();
    assert_eq!(h2.wait().await, WorkflowStatus::Completed);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "post-restart run must hit the persisted idempotency cache and \
         skip the underlying tool call"
    );
}

#[cfg(feature = "job-persist-sqlite")]
mod resume_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::error::WorkflowResumeError;
    use crate::executor::resume::ResumeOptions;
    use crate::sqlite::{WorkflowStorage, compute_spec_hash};

    fn instrumented_caller(counter: Arc<AtomicU32>) -> Arc<MockToolCaller> {
        let m = Arc::new(MockToolCaller::new());
        m.add("a", {
            let c = counter.clone();
            move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"step": "a"}))
            }
        });
        m.add("b", {
            let c = counter.clone();
            move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"step": "b"}))
            }
        });
        m
    }

    fn two_step_spec() -> WorkflowSpec {
        spec_with_steps(vec![
            tool_step("a", "a", Value::Null),
            tool_step("b", "b", Value::Null),
        ])
    }

    #[tokio::test]
    async fn resume_returns_not_found_for_unknown_id() {
        let storage = Arc::new(WorkflowStorage::open_in_memory().unwrap());
        let exe = WorkflowExecutor::builder()
            .tool_caller(Arc::new(MockToolCaller::new()))
            .storage(storage)
            .build();
        let err = exe
            .resume(Uuid::new_v4(), ResumeOptions::default())
            .unwrap_err();
        assert!(matches!(err, WorkflowResumeError::NotFound(_)));
    }

    #[tokio::test]
    async fn resume_skips_already_completed_steps_and_runs_the_rest() {
        let calls = Arc::new(AtomicU32::new(0));
        let storage = Arc::new(WorkflowStorage::open_in_memory().unwrap());

        let exe1 = WorkflowExecutor::builder()
            .tool_caller(instrumented_caller(calls.clone()))
            .storage(Arc::clone(&storage))
            .build();
        let h = exe1.run(two_step_spec(), Value::Null, None).unwrap();
        let workflow_id = h.workflow_id;
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // Force the row back to Failed so it is eligible for resume.
        storage
            .update_workflow_status(workflow_id, WorkflowStatus::Failed, Some("a"))
            .unwrap();
        // Pretend step "a" completed but "b" did not.
        storage
            .upsert_step(workflow_id, "b", "interrupted", None, None)
            .unwrap();

        // Build a fresh executor + caller — counts only resume-time calls.
        let calls2 = Arc::new(AtomicU32::new(0));
        let exe2 = WorkflowExecutor::builder()
            .tool_caller(instrumented_caller(calls2.clone()))
            .storage(Arc::clone(&storage))
            .build();
        let handle = exe2.resume(workflow_id, ResumeOptions::default()).unwrap();
        assert_eq!(handle.wait().await, WorkflowStatus::Completed);
        assert_eq!(
            calls2.load(Ordering::SeqCst),
            1,
            "resume must skip step 'a' (completed) and re-run step 'b' (interrupted) only"
        );
    }

    #[tokio::test]
    async fn resume_force_steps_re_runs_completed_steps() {
        let calls = Arc::new(AtomicU32::new(0));
        let storage = Arc::new(WorkflowStorage::open_in_memory().unwrap());
        let exe1 = WorkflowExecutor::builder()
            .tool_caller(instrumented_caller(calls.clone()))
            .storage(Arc::clone(&storage))
            .build();
        let wid = exe1
            .run(two_step_spec(), Value::Null, None)
            .unwrap()
            .workflow_id;
        // Wait by polling — handle.wait() consumes the join.
        for _ in 0..50 {
            if storage
                .load_resume_snapshot(wid)
                .unwrap()
                .map(|s| s.status == WorkflowStatus::Completed)
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        // Both 'a' and 'b' are completed. Force re-run of 'a'.
        let calls2 = Arc::new(AtomicU32::new(0));
        let exe2 = WorkflowExecutor::builder()
            .tool_caller(instrumented_caller(calls2.clone()))
            .storage(Arc::clone(&storage))
            .build();
        let opts = ResumeOptions {
            force_steps: vec!["a".to_string()],
            ..Default::default()
        };
        let h = exe2.resume(wid, opts).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        assert_eq!(
            calls2.load(Ordering::SeqCst),
            1,
            "force_steps=['a'] must re-run step 'a' but skip 'b' (completed and not forced)"
        );
    }

    #[tokio::test]
    async fn resume_strict_mode_refuses_when_spec_hash_mismatches() {
        let storage = Arc::new(WorkflowStorage::open_in_memory().unwrap());
        let calls = Arc::new(AtomicU32::new(0));
        let exe = WorkflowExecutor::builder()
            .tool_caller(instrumented_caller(calls))
            .storage(Arc::clone(&storage))
            .build();
        let wid = exe
            .run(two_step_spec(), Value::Null, None)
            .unwrap()
            .workflow_id;
        // Wait for terminal.
        for _ in 0..50 {
            if matches!(
                storage.load_resume_snapshot(wid).unwrap().map(|s| s.status),
                Some(WorkflowStatus::Completed)
            ) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        storage
            .update_workflow_status(wid, WorkflowStatus::Failed, None)
            .unwrap();
        let opts = ResumeOptions {
            expected_spec_hash: Some("definitely-not-the-real-hash".to_string()),
            strict: true,
            ..Default::default()
        };
        let err = exe.resume(wid, opts).unwrap_err();
        assert!(matches!(err, WorkflowResumeError::SpecChanged { .. }));
    }

    #[test]
    fn resume_returns_no_storage_when_executor_lacks_storage() {
        let exe = WorkflowExecutor::builder()
            .tool_caller(Arc::new(MockToolCaller::new()))
            .build();
        let err = exe
            .resume(Uuid::new_v4(), ResumeOptions::default())
            .unwrap_err();
        assert!(matches!(err, WorkflowResumeError::NoStorage));
    }

    #[test]
    fn compute_spec_hash_is_deterministic_and_changes_with_spec() {
        let s1 = two_step_spec();
        let s2 = two_step_spec();
        assert_eq!(
            compute_spec_hash(&s1),
            compute_spec_hash(&s2),
            "identical specs must hash identically"
        );
        let s3 = spec_with_steps(vec![tool_step("c", "c", Value::Null)]);
        assert_ne!(
            compute_spec_hash(&s1),
            compute_spec_hash(&s3),
            "different specs must hash differently"
        );
    }
}
