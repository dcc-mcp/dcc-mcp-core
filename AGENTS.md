# AGENTS.md — dcc-mcp-core

> **Navigation map, not a reference manual.**
> Follow the links; don't read everything upfront.
> Detailed rules, traps, and code examples → [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md)

## Response Language

- Reply to the user in **Simplified Chinese** (中文简体) by default.
- Keep all code, identifiers, commit messages, branch names, docstrings,
  comments, and file contents in **English** — this rule governs only the
  conversational/assistant-facing output, not anything written to disk or
  pushed to git.
- If the user explicitly requests another language for a specific reply,
  follow that request for that turn.

## Document Hierarchy

| Layer | File | When to read it |
|-------|------|-----------------|
| Navigation | `AGENTS.md` (this file) | First contact |
| AI-friendly index | `llms.txt` | When you need to *use* APIs |
| Full index | `llms-full.txt` | When `llms.txt` lacks detail |
| Detailed rules | [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md) | Before writing code — traps, do/don't, code style |
| Conceptual docs | `docs/guide/` + `docs/api/` | Building a new adapter or skill |
| Skill authoring | `skills/README.md` + `examples/skills/` | Creating or modifying skills |

---

## Quick Orientation

**What**: Rust-powered MCP library for DCC software (Maya, Blender, Houdini, Photoshop…). PyO3/maturin. Zero Python runtime deps. MCP 2025-03-26 Streamable HTTP.

**API surface** — read in this order:
1. `python/dcc_mcp_core/__init__.py` — every public symbol
2. `python/dcc_mcp_core/_core.pyi` — parameter names and types
3. `llms.txt` — compressed version of (1)+(2)

---

## Decision Tables — Find the Right API

### What do you need?

| Need | Use this |
|------|----------|
| Expose DCC tools over MCP | `DccServerBase` → subclass → `start()` |
| Zero-code tool registration | `SKILL.md` + `scripts/` ([agentskills.io](https://agentskills.io/specification)) |
| Structured results | `success_result()` / `error_result()` |
| Rich error with traceback | `skill_error_with_trace()` |
| Bridge non-Python DCC | `DccBridge` (WebSocket JSON-RPC 2.0) |
| IPC | `IpcChannelAdapter` / `SocketServerAdapter` + `DccLinkFrame` |
| Hand off files between tools | `FileRef` + `artefact_put_file()` / `artefact_get_bytes()` |
| Multi-DCC gateway | `McpHttpConfig(gateway_port=9765)` |
| Gateway failover | `DccGatewayElection(dcc_name, server)` — auto-promote on gateway failure |
| Skill scoping | `SkillScope` (Repo → User → System → Admin) — Rust-only |
| Progressive tool exposure | `SkillGroup` + `activate_tool_group()` |
| Connection-scoped cache | `McpHttpConfig(enable_tool_cache=True)` — per-session `tools/list` snapshot (#438) |
| Instance-bound diagnostics | `DccServerBase(..., dcc_pid=pid)` |
| Remote auth | `ApiKeyConfig` / `OAuthConfig` / `validate_bearer_token` |
| Batch / orchestration | `batch_dispatch()`, `EvalContext`, `DccApiExecutor` |
| Mid-call user input | `elicit_form()` / `elicit_url()` |
| Rich content results | `skill_success_with_chart/table/image` |
| Plugin bundle | `build_plugin_manifest()` / `server.plugin_manifest()` |
| In-process skill execution (embedded DCC) | `SkillCatalog.set_in_process_executor(callable)` |
| Skill scanning | `scan_and_load(dcc_name=...)` → always unpack `(skills, skipped)` tuple |
| Tolerate broken SKILL.md | `scan_and_load_lenient(...)` instead of `scan_and_load` |
| Discover team-level skills | `scan_and_load_team()` / `scan_and_load_team_lenient()` |
| Discover user-level skills | `scan_and_load_user()` / `scan_and_load_user_lenient()` |
| Disable evolved skills | `ENV_DISABLE_ACCUMULATED_SKILLS` |
| MCP HTTP (recommended) | `create_skill_server("maya", McpHttpConfig(port=8765))` |
| MCP HTTP (manual) | `McpHttpServer(registry, McpHttpConfig(port=8765))` |
| Full-screen capture | `Capturer.new_auto().capture()` |
| Single-window capture | `Capturer.new_window_auto().capture_window(...)` |
| Capture DCC output streams | `OutputCapture` — stdout/stderr/script-editor as `output://` resource |
| Cooperative cancellation | `check_cancelled()` in long-running skill scripts |
| Checkpoint/resume | `save_checkpoint(job_id, state)` / `get_checkpoint(job_id)` |
| Agent-facing docs resources | `register_docs_server(server)` → `docs://` MCP resources |
| Agent feedback | `register_feedback_tool(server)` → `dcc_feedback__report` tool |
| Runtime introspection | `register_introspect_tools(server)` → `dcc_introspect__*` tools |
| Skill recipe lookup | `register_recipes_tools(server, skills=...)` |
| YAML workflow definitions | `load_workflow_yaml(path)` / `register_workflow_yaml_tools(server)` |
| Skill hot-reload | `DccSkillHotReloader(dcc_name, server).enable(paths)` |
| Singleton server factory | `make_start_stop(ServerClass)` → `(start_fn, stop_fn)` |
| Skill validation | `validate_skill(skill_dir)` → `SkillValidationReport` |
| Zero-dep JSON/YAML | `json_dumps/loads` / `yaml_dumps/loads` (Rust-powered) |

| `infrastructure` | Safety, diagnostics, introspection |
| `domain` | Pipeline-level intent (shot export, render farm) |
| `example` | Authoring reference only |

---

## AI Agent Tool Priority

1. **Skill Discovery**: `search_skills(query)` → `load_skill(name)` → use tools
2. **Skill-Based Tools**: Validated schemas + `next-tools` + `ToolAnnotations`
3. **Diagnostics**: `diagnostics__screenshot` / `audit_log` / `process_status`
4. **Direct Registry** (last resort): Validate with `ToolValidator` + sandbox with `SandboxPolicy`

---

## Top 5 Traps — Memorize These

1. **`scan_and_load` returns a 2-tuple** → `skills, skipped = scan_and_load(...)` — never iterate directly
2. **`success_result` kwargs → context** → `success_result("msg", count=5)` — do NOT use `context=`
3. **`ToolDispatcher` uses `.dispatch()`** → never `.call()`
4. **Register ALL handlers BEFORE `server.start()`** — server reads registry at startup
5. **SKILL.md extensions use `metadata.dcc-mcp.<feature>`** → sibling files, never top-level keys (v0.15+ / #356)

Full trap list + code examples → [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md)

---

## Build & Test

`vx just dev` (build wheel) → `vx just test` → `vx just preflight` (pre-commit check + docs dead-link check)

---

## Repo Layout (What Lives Where)

```
crates/          # Rust — 15 crates
python/dcc_mcp_core/__init__.py  # ← every public symbol
python/dcc_mcp_core/_core.pyi   # ← parameter names & types
tests/           # 120+ integration tests
examples/skills/ # 11 complete SKILL.md packages
docs/            # guides + API reference
```

---

## Essential Do / Don't

### Do ✅
- Use `create_skill_server()` — Skills-First entry point
- Use `success_result("msg", count=5)` — kwargs become context
- Use `ToolAnnotations` — safety hints for AI clients
- Use `search_skills(query)` — don't guess tool names
- Use `metadata.dcc-mcp.<feature>` keys + sibling files for all SKILL.md extensions
- Tag every skill with `metadata.dcc-mcp.layer`
- Unpack `scan_and_load()`: `skills, skipped = scan_and_load(...)`
- Use Conventional Commits: `feat:`, `fix:`, `docs:`, `refactor:`
- Use `vx just dev` before `vx just test`

### Don't ❌ (and what to do instead)
- Don't iterate `scan_and_load()` → **unpack the 2-tuple**
- Don't use `context=` kwarg in `success_result()` → **pass kwargs directly**
- Don't call `ToolDispatcher.call()` → **use `.dispatch(name, json_str)`**
- Don't put SKILL.md extensions at top level → **use `metadata.dcc-mcp.<feature>` + sibling file**
- Don't add Python runtime deps → **zero-dep by design**
- Don't manually bump versions → **Release Please handles this**
- Don't import `SkillScope` from Python → **it's Rust-only; use `SkillMetadata` methods**

Full list with code examples → [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md)

---

## External Standards

| What | Where |
|------|-------|
| MCP spec (2025-03-26) | https://modelcontextprotocol.io/specification/2025-03-26 |
| SKILL.md format | https://agentskills.io/specification |
| AGENTS.md standard | https://agents.md/ |
| llms.txt format | https://llmstxt.org/ |
