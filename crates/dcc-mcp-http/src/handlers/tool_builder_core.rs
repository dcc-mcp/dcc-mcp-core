// ── Core tool definitions ─────────────────────────────────────────────────

/// Process-global cache for the core discovery tools.
///
/// The core tools (`find_skills`, `load_skill`, `unload_skill`, `get_skill_info`,
/// `search_skills`) have static schemas that never change at runtime.  We build
/// them once on the first `tools/list` call and reuse the result for every
/// subsequent request, eliminating a handful of `String::from` / `json!` allocations
/// per request.
use super::*;

static CORE_TOOLS_CACHE: OnceLock<Vec<McpTool>> = OnceLock::new();

/// Return the core discovery tools, building and caching them on the first call.
pub fn build_core_tools() -> &'static [McpTool] {
    CORE_TOOLS_CACHE.get_or_init(build_core_tools_inner)
}

/// Inner builder — called exactly once per process lifetime.
pub fn build_core_tools_inner() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "list_roots".to_string(),
            description: "Returns the filesystem roots the MCP client advertised for this session (cached from roots/list).\n\n\
                          When to use: Call when another tool needs to resolve a relative path and you have no absolute context yet. Rarely needed — most DCC tools operate on in-memory scene data, not the client's workspace.\n\n\
                          How to use:\n\
                          - Takes no arguments; returns an empty array if the client sent no roots.\n\
                          - Do not call repeatedly; roots change only on client reconnect."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("List Roots".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "find_skills".to_string(),
            description: "Deprecated (#340): forwards to search_skills and stamps _meta with a deprecation notice; removed in v0.17.\n\n\
                          When to use: Only for backward compatibility. New code should call search_skills instead.\n\n\
                          How to use:\n\
                          - Prefer search_skills(query, tags, dcc, scope, limit).\n\
                          - After a match, call load_skill(skill_name=...)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keyword matched against skill name and description."
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Tag filter; every listed tag must match."
                    },
                    "dcc": {
                        "type": "string",
                        "description": "DCC type filter (e.g. maya, blender, houdini)."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Find Skills (deprecated)".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "list_skills".to_string(),
            description: "Lists every discovered skill on this server with its current load status (loaded, unloaded, or error).\n\n\
                          When to use: Use to browse what is available or to audit which skills are currently active. For keyword lookup, call search_skills instead — list_skills is a flat dump with no ranking.\n\n\
                          How to use:\n\
                          - Pass status='loaded' to inspect the active tool surface, 'unloaded' to find candidates to load.\n\
                          - Follow up with get_skill_info(skill_name=...) or load_skill(skill_name=...)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["all", "loaded", "unloaded", "error"],
                        "default": "all",
                        "description": "Load-status filter; 'all' returns every discovered skill."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("List Skills".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "get_skill_info".to_string(),
            description: "Returns detailed metadata for one skill: description, tags, DCC binding, and full input schemas for every tool it declares.\n\n\
                          When to use: Use when you already know the skill name and need to inspect its tools' schemas before committing to load_skill. Pair this with search_skills when deciding between candidates.\n\n\
                          How to use:\n\
                          - Inspecting alone does not make the tools callable; follow up with load_skill(skill_name=...) to activate them.\n\
                          - Returns an error if the skill is unknown."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Exact skill name as reported by list_skills / search_skills."
                    }
                },
                "required": ["skill_name"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Get Skill Info".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "load_skill".to_string(),
            description: "Loads one or more discovered skills and registers their tools, then emits a tools/list_changed notification.\n\n\
                          When to use: Call after search_skills, list_skills, or get_skill_info has identified the skill you need. Idempotent — re-loading an already-loaded skill is a no-op.\n\n\
                          How to use:\n\
                          - Use skill_name for one skill, or skill_names for a batch in a single round-trip.\n\
                          - After success, call tools/list or the specific tool (e.g. maya_geometry__create_sphere) directly."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Single skill to load; mutually exclusive with skill_names."
                    },
                    "skill_names": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Batch of skill names to load in one call."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Load Skill".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "unload_skill".to_string(),
            description: "Unloads a previously loaded skill, unregisters its tools, and emits a tools/list_changed notification.\n\n\
                          When to use: Use to free tool slots and shrink the tools/list token footprint once a workflow no longer needs a skill. Safe to call on an unloaded skill (no-op).\n\n\
                          How to use:\n\
                          - Pending tools/call requests against this skill will fail after unload — drain them first.\n\
                          - To re-enable the skill later, call load_skill(skill_name=...)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Exact skill name previously passed to load_skill."
                    }
                },
                "required": ["skill_name"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Unload Skill".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "search_skills".to_string(),
            description: "Unified skill discovery (#340, supersedes find_skills). Ranks skills against query across name, description, search-hint, tags, and tool names; filters by tags/dcc/scope.\n\n\
                          When to use: Start here when you need a capability but don't know the skill name. Call with no args to browse by trust scope (Admin>System>User>Repo).\n\n\
                          How to use:\n\
                          - Keep query short (2-4 keywords); combine with tags/dcc/scope.\n\
                          - After a hit, call load_skill(skill_name=...)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Short keyword phrase (2-4 words). Leave empty to browse by scope."
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter by tags (all must match; case-insensitive)."
                    },
                    "dcc": {
                        "type": "string",
                        "description": "DCC filter (e.g. maya, blender, houdini)."
                    },
                    "scope": {
                        "type": "string",
                        "enum": ["repo", "user", "system", "admin"],
                        "description": "Filter by trust scope (Admin > System > User > Repo)."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "default": 20,
                        "description": "Cap the number of results (default 20, max 100)."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Search Skills".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "activate_tool_group".to_string(),
            description: "Activates a tool group inside a loaded skill, making its members callable and emitting a tools/list_changed notification.\n\n\
                          When to use: Call when tools/list surfaces a __group__<name> stub and you need the underlying tools. Progressive exposure keeps the default surface small until you opt in.\n\n\
                          How to use:\n\
                          - The parent skill must be loaded first; check list_skills(status='loaded') if unsure.\n\
                          - After activation, re-run tools/list to see the newly available tools, then call them by name."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "Group name as shown in the __group__<name> stub."
                    }
                },
                "required": ["group"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Activate Tool Group".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "deactivate_tool_group".to_string(),
            description: "Deactivates a tool group, collapsing its members back into a __group__<name> stub and emitting a tools/list_changed notification.\n\n\
                          When to use: Use to shrink the active tool surface once a sub-workflow is done, to stay within the client's token budget. Group tools remain on disk — only their visibility changes.\n\n\
                          How to use:\n\
                          - Idempotent; calling on an already-inactive group is a safe no-op.\n\
                          - To bring the tools back, call activate_tool_group(group=...)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "Group name previously passed to activate_tool_group."
                    }
                },
                "required": ["group"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Deactivate Tool Group".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "search_tools".to_string(),
            description: "Full-text search over already-registered tools, matching name, description, category, and tags and ranking enabled tools first.\n\n\
                          When to use: Use after skills are loaded to locate a specific tool without dumping the whole tools/list. If nothing matches, fall back to search_skills — the tool may live in an unloaded skill.\n\n\
                          How to use:\n\
                          - Keep the query short; set include_disabled=true only when inspecting inactive groups.\n\
                          - Call the returned tool directly by its name."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keyword matched against tool name, description, category, and tags."
                    },
                    "dcc": {
                        "type": "string",
                        "description": "DCC filter (e.g. maya, blender)."
                    },
                    "include_disabled": {
                        "type": "boolean",
                        "default": false,
                        "description": "Also search tools inside inactive tool groups."
                    }
                },
                "required": ["query"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Search Tools".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        // `jobs.get_status` — built-in job-polling tool (#319).
        //
        // Complements the `$/dcc.jobUpdated` SSE channel (#326) for clients
        // that prefer request/response polling over a long-lived stream.
        // SEP-986 compliant: the dot-separated `jobs.*` namespace is the
        // reserved built-in prefix (see `docs/guide/naming.md`). We panic
        // at first build if the regex or the length cap ever rejects this
        // name — that would be a dcc-mcp-naming regression and we want to
        // catch it loudly.
        {
            const TOOL_NAME: &str = "jobs.get_status";
            if let Err(e) = dcc_mcp_naming::validate_tool_name(TOOL_NAME) {
                panic!("built-in tool name `{TOOL_NAME}` fails SEP-986 validation: {e}");
            }
            McpTool {
                name: TOOL_NAME.to_string(),
                description: "Poll the status of an async tool-call job tracked by JobManager. \
                              Returns a JSON envelope with job_id, parent_job_id, tool, status \
                              (pending|running|completed|failed|cancelled|interrupted), timestamps, \
                              progress, error, and optionally the final ToolResult once the job \
                              is terminal. Complements the `$/dcc.jobUpdated` SSE channel (#326) \
                              for polling-based clients. Returns isError=true with a human-readable \
                              message when the job id is unknown (never a JSON-RPC transport error)."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "job_id": {
                            "type": "string",
                            "description": "UUID of the job to query"
                        },
                        "include_logs": {
                            "type": "boolean",
                            "default": false,
                            "description": "Include captured stdout/stderr if any. \
                                            Currently a no-op — JobManager does not capture logs; \
                                            the flag is accepted for forward compatibility."
                        },
                        "include_result": {
                            "type": "boolean",
                            "default": true,
                            "description": "Include the job's final ToolResult when the job is \
                                            in a terminal state (completed/failed). Ignored for \
                                            pending/running jobs since no result exists yet."
                        }
                    },
                    "required": ["job_id"]
                }),
                output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Get Job Status".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
            }
        },
        // `jobs.cleanup` — built-in TTL pruning tool (#328). Removes
        // terminal job rows (and storage-backed rows when a
        // `job_storage_path` is configured) older than the given
        // window. Never touches pending / running jobs.
        {
            const TOOL_NAME: &str = "jobs.cleanup";
            if let Err(e) = dcc_mcp_naming::validate_tool_name(TOOL_NAME) {
                panic!("built-in tool name `{TOOL_NAME}` fails SEP-986 validation: {e}");
            }
            McpTool {
                name: TOOL_NAME.to_string(),
                description: "Purge terminal (completed/failed/cancelled/interrupted) jobs \
                              older than `older_than_hours` hours from JobManager and any \
                              attached storage backend. Non-terminal (pending/running) jobs \
                              are never removed regardless of age. Returns {removed: <count>} \
                              as structured content. Idempotent — repeated calls with the \
                              same window return 0 once the pruning horizon is reached."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "older_than_hours": {
                            "type": "integer",
                            "minimum": 0,
                            "default": 24,
                            "description": "Prune terminal jobs whose last update is older \
                                            than this many hours. Default: 24."
                        }
                    },
                    "required": []
                }),
                output_schema: None,
                annotations: Some(McpToolAnnotations {
                    title: Some("Cleanup Completed Jobs".to_string()),
                    read_only_hint: Some(false),
                    destructive_hint: Some(true),
                    idempotent_hint: Some(true),
                    open_world_hint: Some(false),
                    deferred_hint: Some(false),
                }),
                meta: None,
            }
        },
    ]
}

/// Build the three opt-in meta-tools for the lazy-actions fast-path (#254).
///
/// All three tool names are bare, lower-snake and ≤ 16 chars — SEP-986
/// compliant and therefore legal to surface unprefixed in `tools/list`.
