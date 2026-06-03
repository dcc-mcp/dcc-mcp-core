use super::*;
use serde_json::json;

#[test]
fn payload_truncates_at_cap() {
    let big = json!({"data": "a".repeat(100)});
    let p = TracePayload::from_value(&big, 50);
    assert!(p.truncated);
    assert!(p.content.len() <= 50);
    assert!(p.original_size > 50);
}

#[test]
fn payload_estimates_tokens_for_json() {
    let p = TracePayload::from_value(&json!("hello world"), 1024);
    assert!(p.estimated_tokens.is_some());
    assert!(p.estimated_tokens.unwrap() > 0);
}

#[test]
fn payload_no_truncation_when_under_cap() {
    let small = json!({"x": 1});
    let p = TracePayload::from_value(&small, 1024);
    assert!(!p.truncated);
    assert_eq!(p.original_size, p.content.len());
}

#[test]
fn input_payload_redacts_script_source_fields() {
    let raw = json!({
        "tool_slug": "maya.abc.execute_python",
        "arguments": {
            "code": "print('secret')",
            "nested": {
                "content": "raw script body"
            },
            "file_path": "/tmp/materialized.py"
        }
    });

    let p = TracePayload::from_input_value(&raw, 4096);

    assert!(p.content.contains("[REDACTED_SCRIPT_SOURCE]"));
    assert!(!p.content.contains("print('secret')"));
    assert!(!p.content.contains("raw script body"));
    assert!(p.content.contains("/tmp/materialized.py"));
}

#[test]
fn agent_context_reads_meta_and_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("x-dcc-mcp-actor-id", "user-7".parse().unwrap());
    headers.insert("x-dcc-mcp-actor-name", "Morgan Artist".parse().unwrap());
    headers.insert("x-dcc-mcp-agent-id", "agent-7".parse().unwrap());
    headers.insert("x-dcc-mcp-agent-version", "0.9.0".parse().unwrap());
    headers.insert("x-dcc-mcp-agent-model", "gpt-test".parse().unwrap());
    headers.insert("x-dcc-mcp-agent-model-provider", "openai".parse().unwrap());
    headers.insert("x-dcc-mcp-agent-turn-id", "turn-9".parse().unwrap());
    headers.insert("x-dcc-mcp-client-platform", "custom-http".parse().unwrap());
    headers.insert("x-dcc-mcp-client-os", "windows".parse().unwrap());
    headers.insert("x-dcc-mcp-auth-subject", "apikey:team-a".parse().unwrap());
    headers.insert("x-dcc-mcp-user-input-chars", "2500".parse().unwrap());
    let meta = json!({
        "agent_context": {
            "actorEmailHash": "sha256:actor",
            "agent_name": "Scene Planner",
            "modelVersion": "gpt-5.1",
            "reasoningEffort": "medium",
            "sessionId": "session-meta",
            "task": "inspect material bindings",
            "userIntentSummary": "Inspect scene before editing.",
            "agentReplySummary": "I will inspect the scene graph first.",
            "userInputHash": "sha256:user",
            "agentReplyHash": "sha256:reply",
            "agentReplyChars": 140,
            "reasoning_summary": "Need a lightweight scene read before edit.",
            "plan": ["describe scene", "choose material patch"]
        }
    });

    let ctx = AgentContext::from_request_parts(&headers, None, Some(&meta)).unwrap();

    assert_eq!(ctx.actor_id.as_deref(), Some("user-7"));
    assert_eq!(ctx.actor_name.as_deref(), Some("Morgan Artist"));
    assert_eq!(ctx.actor_email_hash.as_deref(), Some("sha256:actor"));
    assert_eq!(ctx.agent_id.as_deref(), Some("agent-7"));
    assert_eq!(ctx.agent_name.as_deref(), Some("Scene Planner"));
    assert_eq!(ctx.agent_version.as_deref(), Some("0.9.0"));
    assert_eq!(ctx.model.as_deref(), Some("gpt-test"));
    assert_eq!(ctx.model_provider.as_deref(), Some("openai"));
    assert_eq!(ctx.model_version.as_deref(), Some("gpt-5.1"));
    assert_eq!(ctx.reasoning_effort.as_deref(), Some("medium"));
    assert_eq!(ctx.session_id.as_deref(), Some("session-meta"));
    assert_eq!(ctx.turn_id.as_deref(), Some("turn-9"));
    assert_eq!(
        ctx.user_intent_summary.as_deref(),
        Some("Inspect scene before editing.")
    );
    assert_eq!(
        ctx.agent_reply_summary.as_deref(),
        Some("I will inspect the scene graph first.")
    );
    assert_eq!(ctx.client_platform.as_deref(), Some("custom-http"));
    assert_eq!(ctx.client_os.as_deref(), Some("windows"));
    assert_eq!(ctx.auth_subject.as_deref(), Some("apikey:team-a"));
    assert_eq!(ctx.user_input_hash.as_deref(), Some("sha256:user"));
    assert_eq!(ctx.agent_reply_hash.as_deref(), Some("sha256:reply"));
    assert_eq!(ctx.user_input_chars, Some(2500));
    assert_eq!(ctx.agent_reply_chars, Some(140));
    assert_eq!(ctx.plan.len(), 2);
    assert_eq!(ctx.display_name(), Some("Morgan Artist"));
    assert_eq!(ctx.trust.actor_id.as_deref(), Some(TRUST_HEADER));
    assert_eq!(ctx.trust.actor_name.as_deref(), Some(TRUST_HEADER));
    assert_eq!(
        ctx.trust.actor_email_hash.as_deref(),
        Some(TRUST_SELF_REPORTED)
    );
    assert_eq!(ctx.trust.agent_id.as_deref(), Some(TRUST_HEADER));
    assert_eq!(ctx.trust.agent_name.as_deref(), Some(TRUST_SELF_REPORTED));
    assert_eq!(ctx.trust.model.as_deref(), Some(TRUST_HEADER));
    assert_eq!(
        ctx.trust.model_version.as_deref(),
        Some(TRUST_SELF_REPORTED)
    );
    assert_eq!(ctx.trust.client_platform.as_deref(), Some(TRUST_HEADER));
    assert_eq!(ctx.trust.auth_subject.as_deref(), Some(TRUST_HEADER));
}

#[test]
fn agent_context_reads_mcp_initialize_client_info() {
    let ctx = AgentContext::from_mcp_client_info(
        "mcp-session-1",
        Some(&json!({
            "protocolVersion": "2025-03-26",
            "clientInfo": {"name": "Codex Desktop", "version": "1.2.3"}
        })),
    )
    .unwrap();

    assert_eq!(ctx.agent_name.as_deref(), Some("Codex Desktop"));
    assert_eq!(ctx.agent_kind.as_deref(), Some("mcp-client"));
    assert_eq!(ctx.agent_version.as_deref(), Some("1.2.3"));
    assert_eq!(ctx.client_platform.as_deref(), Some("Codex Desktop"));
    assert_eq!(ctx.session_id.as_deref(), Some("mcp-session-1"));
    assert_eq!(ctx.trust.agent_name.as_deref(), Some(TRUST_SELF_REPORTED));
    assert_eq!(
        ctx.trust.client_platform.as_deref(),
        Some(TRUST_SELF_REPORTED)
    );
}

#[test]
fn agent_context_reads_agent_alias_and_user_agent_platform() {
    let mut headers = HeaderMap::new();
    headers.insert("x-dcc-mcp-agent", "Studio Gateway CLI".parse().unwrap());
    headers.insert("user-agent", "dcc-mcp-cli/0.17.37 reqwest".parse().unwrap());

    let ctx = AgentContext::from_request_parts(&headers, None, None).unwrap();

    assert_eq!(ctx.agent_name.as_deref(), Some("Studio Gateway CLI"));
    assert_eq!(ctx.client_platform.as_deref(), Some("dcc-mcp-cli"));
    assert_eq!(ctx.trust.agent_name.as_deref(), Some(TRUST_HEADER));
    assert_eq!(ctx.trust.client_platform.as_deref(), Some(TRUST_HEADER));
}

#[test]
fn agent_context_accepts_plain_summary() {
    let headers = HeaderMap::new();
    let body = json!({"caller_context": "manual smoke test"});

    let ctx = AgentContext::from_request_parts(&headers, Some(&body), None).unwrap();

    assert_eq!(ctx.reasoning_summary.as_deref(), Some("manual smoke test"));
    assert_eq!(ctx.display_name(), None);
}

#[test]
fn caller_attribution_handles_missing_partial_and_malformed_metadata() {
    let headers = HeaderMap::new();
    assert!(AgentContext::from_request_parts(&headers, None, None).is_none());

    let partial = json!({
        "caller_context": {
            "actor_id": "artist-1"
        }
    });
    let partial_ctx = AgentContext::from_request_parts(&headers, Some(&partial), None).unwrap();
    assert_eq!(partial_ctx.actor_id.as_deref(), Some("artist-1"));
    assert_eq!(partial_ctx.agent_id, None);
    assert_eq!(partial_ctx.source_ip, None);
    assert_eq!(
        partial_ctx.trust.actor_id.as_deref(),
        Some(TRUST_SELF_REPORTED)
    );

    let mut header_fallback = HeaderMap::new();
    header_fallback.insert("x-dcc-mcp-client-platform", "studio-tool".parse().unwrap());
    let malformed = json!({
        "caller_context": {
            "actor_id": { "nested": "not a string" }
        }
    });
    let ctx = AgentContext::from_request_parts(&header_fallback, Some(&malformed), None).unwrap();
    assert_eq!(ctx.actor_id, None);
    assert_eq!(ctx.client_platform.as_deref(), Some("studio-tool"));
    assert_eq!(ctx.trust.client_platform.as_deref(), Some(TRUST_HEADER));
}

#[test]
fn caller_attribution_bounds_fields_and_ignores_client_network_source() {
    let headers = HeaderMap::new();
    let long_actor = "artist".repeat(MAX_AGENT_CONTEXT_STRING_BYTES);
    let body = json!({
        "caller_context": {
            "actorId": long_actor,
            "actorName": "Morgan Artist",
            "agentId": "agent-camel",
            "agentVersion": "1.2.3",
            "agentModel": "gpt-test",
            "clientPlatform": "cursor",
            "clientOs": "macos",
            "clientHost": "workstation-42",
            "authSubject": "oauth:user-7",
            "actorEmail": "morgan@example.invalid",
            "sourceIp": "203.0.113.99",
            "forwardedFor": ["198.51.100.10"]
        }
    });

    let ctx = AgentContext::from_request_parts(&headers, Some(&body), None).unwrap();
    let encoded = serde_json::to_string(&ctx).unwrap();

    assert!(
        ctx.actor_id.as_ref().unwrap().len() <= MAX_AGENT_CONTEXT_STRING_BYTES,
        "actor_id should be bounded"
    );
    assert_eq!(ctx.actor_name.as_deref(), Some("Morgan Artist"));
    assert_eq!(ctx.agent_id.as_deref(), Some("agent-camel"));
    assert_eq!(ctx.agent_version.as_deref(), Some("1.2.3"));
    assert_eq!(ctx.model.as_deref(), Some("gpt-test"));
    assert_eq!(ctx.client_platform.as_deref(), Some("cursor"));
    assert_eq!(ctx.client_os.as_deref(), Some("macos"));
    assert_eq!(ctx.client_host.as_deref(), Some("workstation-42"));
    assert_eq!(ctx.auth_subject.as_deref(), Some("oauth:user-7"));
    assert_eq!(ctx.source_ip, None, "source_ip must be server-derived");
    assert!(
        ctx.forwarded_for.is_empty(),
        "forwarded_for must be server-derived"
    );
    assert_eq!(ctx.trust.actor_id.as_deref(), Some(TRUST_SELF_REPORTED));
    assert_eq!(
        ctx.trust.client_platform.as_deref(),
        Some(TRUST_SELF_REPORTED)
    );
    assert_eq!(ctx.trust.auth_subject.as_deref(), Some(TRUST_SELF_REPORTED));
    assert!(ctx.trust.source_ip.is_none());
    assert!(!encoded.contains("morgan@example.invalid"));
}

#[test]
fn caller_attribution_network_source_can_be_added_by_server_boundary() {
    let ctx = AgentContext {
        actor_id: Some("user-7".to_string()),
        ..AgentContext::default()
    }
    .with_server_network_source(
        Some("192.0.2.44".to_string()),
        vec!["198.51.100.2".to_string(), "203.0.113.3".to_string()],
    );

    assert_eq!(ctx.source_ip.as_deref(), Some("192.0.2.44"));
    assert_eq!(
        ctx.forwarded_for,
        vec!["198.51.100.2".to_string(), "203.0.113.3".to_string()]
    );
    assert_eq!(ctx.trust.source_ip.as_deref(), Some(TRUST_TRUSTED_PROXY));
    assert_eq!(
        ctx.trust.forwarded_for.as_deref(),
        Some(TRUST_TRUSTED_PROXY)
    );
}

#[test]
fn caller_attribution_reads_only_internal_server_network_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("x-dcc-mcp-source-ip", "203.0.113.99".parse().unwrap());
    headers.insert(
        crate::gateway::caller_attribution::INTERNAL_SOURCE_IP_HEADER,
        "192.0.2.44".parse().unwrap(),
    );
    headers.insert(
        crate::gateway::caller_attribution::INTERNAL_FORWARDED_FOR_HEADER,
        "198.51.100.2, 203.0.113.3".parse().unwrap(),
    );
    let body = json!({
        "caller_context": {
            "actor_id": "artist-1",
            "sourceIp": "203.0.113.100"
        }
    });

    let ctx =
        AgentContext::from_request_parts_with_server_network(&headers, Some(&body), None).unwrap();

    assert_eq!(ctx.actor_id.as_deref(), Some("artist-1"));
    assert_eq!(ctx.source_ip.as_deref(), Some("192.0.2.44"));
    assert_eq!(
        ctx.forwarded_for,
        vec!["198.51.100.2".to_string(), "203.0.113.3".to_string()]
    );
    assert_eq!(ctx.trust.actor_id.as_deref(), Some(TRUST_SELF_REPORTED));
    assert_eq!(ctx.trust.source_ip.as_deref(), Some(TRUST_TRUSTED_PROXY));
}

#[test]
fn caller_attribution_auth_subject_prefers_internal_auth_boundary() {
    let mut headers = HeaderMap::new();
    headers.insert("x-dcc-mcp-auth-subject", "header:spoofed".parse().unwrap());
    headers.insert(
        crate::gateway::caller_attribution::INTERNAL_AUTH_SUBJECT_HEADER,
        "oauth:artist-1".parse().unwrap(),
    );
    let body = json!({
        "caller_context": {
            "authSubject": "body:spoofed"
        }
    });

    let ctx = AgentContext::from_request_parts(&headers, Some(&body), None).unwrap();

    assert_eq!(ctx.auth_subject.as_deref(), Some("oauth:artist-1"));
    assert_eq!(ctx.trust.auth_subject.as_deref(), Some(TRUST_AUTH));
}

#[test]
fn caller_attribution_ignores_mcp_meta_network_spoofing() {
    let mut headers = HeaderMap::new();
    headers.insert(
        crate::gateway::caller_attribution::INTERNAL_SOURCE_IP_HEADER,
        "192.0.2.44".parse().unwrap(),
    );
    let meta = json!({
        "agent_context": {
            "actor_id": "artist-1",
            "sourceIp": "203.0.113.200",
            "forwardedFor": ["203.0.113.201"]
        }
    });

    let ctx =
        AgentContext::from_request_parts_with_server_network(&headers, None, Some(&meta)).unwrap();

    assert_eq!(ctx.actor_id.as_deref(), Some("artist-1"));
    assert_eq!(ctx.source_ip.as_deref(), Some("192.0.2.44"));
    assert!(ctx.forwarded_for.is_empty());
    assert_eq!(ctx.trust.actor_id.as_deref(), Some(TRUST_SELF_REPORTED));
    assert_eq!(ctx.trust.source_ip.as_deref(), Some(TRUST_SERVER_DERIVED));
}

#[test]
fn agent_context_bounds_turn_summaries_and_excludes_raw_text() {
    let headers = HeaderMap::new();
    let raw_prompt = "secret production prompt".to_string();
    let long_summary = "summary ".repeat(MAX_AGENT_CONTEXT_STRING_BYTES);
    let body = json!({
        "caller_context": {
            "agent_id": "agent-raw",
            "turnId": "turn-raw",
            "userIntentSummary": long_summary,
            "user_input": raw_prompt,
            "agentReply": "raw reply should not be stored",
            "metadata": {
                "workflow_id": "wf-1",
                "prompt": "raw prompt in metadata",
                "nested": {
                    "rawAgentReply": "raw reply in nested metadata",
                    "safe": "kept"
                }
            }
        }
    });

    let ctx = AgentContext::from_request_parts(&headers, Some(&body), None).unwrap();
    let encoded = serde_json::to_string(&ctx).unwrap();

    assert!(ctx.user_intent_summary.unwrap().len() <= MAX_AGENT_CONTEXT_STRING_BYTES);
    assert!(!encoded.contains("secret production prompt"));
    assert!(!encoded.contains("raw reply should not be stored"));
    assert!(!encoded.contains("raw prompt in metadata"));
    assert!(!encoded.contains("raw reply in nested metadata"));
    assert_eq!(ctx.metadata["workflow_id"], "wf-1");
    assert_eq!(ctx.metadata["nested"]["safe"], "kept");
    assert_eq!(ctx.metadata["redacted_high_sensitivity_fields"], 1);
    assert_eq!(
        ctx.metadata["nested"]["redacted_high_sensitivity_fields"],
        1
    );
}

#[test]
fn trace_context_parses_w3c_traceparent_without_using_it_as_request_id() {
    let mut headers = HeaderMap::new();
    headers.insert("x-request-id", "req-explicit".parse().unwrap());
    headers.insert(
        "traceparent",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
            .parse()
            .unwrap(),
    );
    headers.insert("tracestate", "vendor=value".parse().unwrap());

    let ctx = TraceContext::from_headers(&headers);

    assert_eq!(ctx.request_id, "req-explicit");
    assert_eq!(ctx.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
    assert_eq!(ctx.parent_span_id.as_deref(), Some("00f067aa0ba902b7"));
    assert_eq!(ctx.trace_flags.as_deref(), Some("01"));
    assert_eq!(ctx.trace_state.as_deref(), Some("vendor=value"));
}

#[test]
fn trace_context_generates_ids_when_headers_are_absent() {
    let ctx = TraceContext::from_headers(&HeaderMap::new());

    assert_eq!(ctx.trace_id.len(), 32);
    assert_eq!(ctx.span_id.as_deref().unwrap_or_default().len(), 16);
    assert!(!ctx.request_id.is_empty());
    assert!(ctx.traceparent().is_some());
}

#[test]
fn trace_context_child_request_preserves_trace_with_distinct_request_id() {
    let mut headers = HeaderMap::new();
    headers.insert("x-request-id", "batch-parent".parse().unwrap());
    headers.insert(
        "traceparent",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
            .parse()
            .unwrap(),
    );
    let parent = TraceContext::from_headers(&headers);
    let child = parent.child_request("batch-parent:batch-0");

    assert_eq!(child.trace_id, parent.trace_id);
    assert_eq!(child.request_id, "batch-parent:batch-0");
    assert_eq!(child.parent_request_id.as_deref(), Some("batch-parent"));
    assert_ne!(child.span_id, parent.span_id);
    assert_eq!(child.parent_span_id, parent.span_id);
}

#[test]
fn trace_log_evicts_oldest_at_capacity() {
    let log = TraceLog::new(3);
    for i in 0u32..5 {
        log.push(DispatchTrace {
            request_id: format!("req-{i}"),
            trace_id: "trace-ring".into(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: None,
            instance_id: None,
            session_id: None,
            dcc_type: None,
            transport: None,
            agent_context: None,
            started_at: SystemTime::now(),
            total_ms: i as u64,
            ok: true,
            spans: vec![],
            input: None,
            output: None,
            token_accounting: None,
            llm_usage: None,
        });
    }
    let recent = log.recent(10);
    assert_eq!(recent.len(), 3);
    // Newest first.
    assert_eq!(recent[0].request_id, "req-4");
    assert_eq!(recent[2].request_id, "req-2");
}

#[test]
fn trace_log_get_by_request_id() {
    let log = TraceLog::new(10);
    log.push(DispatchTrace {
        request_id: "abc-123".into(),
        trace_id: "trace-abc".into(),
        span_id: None,
        parent_span_id: None,
        parent_request_id: None,
        trace_flags: None,
        trace_state: None,
        method: "tools/call".into(),
        tool_slug: Some("maya.create_sphere".into()),
        instance_id: None,
        session_id: None,
        dcc_type: Some("maya".into()),
        transport: None,
        agent_context: None,
        started_at: SystemTime::now(),
        total_ms: 42,
        ok: true,
        spans: vec![],
        input: None,
        output: None,
        token_accounting: None,
        llm_usage: None,
    });
    let found = log.get("abc-123");
    assert!(found.is_some());
    assert_eq!(
        found.unwrap().tool_slug.as_deref(),
        Some("maya.create_sphere")
    );
    assert!(log.get("unknown").is_none());
}

// ── Property-based tests (#846) ────────────────────────────────────────

use proptest::prelude::*;

fn arb_trace(idx: u32) -> DispatchTrace {
    DispatchTrace {
        request_id: format!("req-{idx}"),
        trace_id: format!("trace-{idx}"),
        span_id: None,
        parent_span_id: None,
        parent_request_id: None,
        trace_flags: None,
        trace_state: None,
        method: "tools/call".into(),
        tool_slug: None,
        instance_id: None,
        session_id: None,
        dcc_type: None,
        transport: None,
        agent_context: None,
        started_at: SystemTime::now(),
        total_ms: idx as u64,
        ok: true,
        spans: vec![],
        input: None,
        output: None,
        token_accounting: None,
        llm_usage: None,
    }
}

proptest! {
    /// Ring-buffer law: after pushing `pushes` traces into a buffer of
    /// capacity `capacity`, `recent(usize::MAX).len() == min(pushes, capacity)`.
    /// Proves the buffer never exceeds capacity (memory bound) and never
    /// drops more than necessary.
    #[test]
    fn prop_trace_log_capacity_is_respected(
        capacity in 1usize..32,
        pushes in 0u32..64,
    ) {
        let log = TraceLog::new(capacity);
        for i in 0..pushes {
            log.push(arb_trace(i));
        }
        let recent = log.recent(usize::MAX);
        let expected = (pushes as usize).min(capacity);
        prop_assert_eq!(recent.len(), expected);
    }

    /// Ring-buffer law: `recent(limit)` always returns ≤ `limit` items
    /// and ≤ buffer occupancy. First item is the most recently pushed
    /// trace (LIFO order).
    #[test]
    fn prop_trace_log_recent_returns_newest_first(
        capacity in 1usize..16,
        pushes in 1u32..32,
        limit in 1usize..32,
    ) {
        let log = TraceLog::new(capacity);
        for i in 0..pushes {
            log.push(arb_trace(i));
        }
        let recent = log.recent(limit);
        let bound = limit.min((pushes as usize).min(capacity));
        prop_assert_eq!(recent.len(), bound);
        if !recent.is_empty() {
            prop_assert_eq!(
                &recent[0].request_id,
                &format!("req-{}", pushes - 1)
            );
        }
    }
}
