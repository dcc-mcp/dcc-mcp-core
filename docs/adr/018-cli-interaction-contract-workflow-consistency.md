# ADR 018 — CLI Interaction Contract & Workflow Consistency Design

**Status**: Proposed
**Date**: 2026-07-25
**Relates to**: PIP-2880 (Unity CLI research), PIP-2881 (this RFC)

---

## Context

PIP-2880 surveyed three Unity CLI projects (bigdra50/unity-cli, youngwoocho02/unity-cli,
Unity official CLI) and identified recurring design patterns that our CLI can adopt.
The survey surfaced six concrete recommendations across three priority levels.

Currently `dcc-mcp-cli` has grown organically — output formatting, error reporting,
instance state vocabulary, and destructive-action boundaries vary across commands.
Agents and human operators need a consistent interaction contract to reason about CLI
behavior without inspecting source code.

This ADR formalizes the P0/P1/P2 recommendations from PIP-2880 into actionable
design decisions for `dcc-mcp-cli`.

---

## Decision

### P0 — CLI Output Contract (Non-Negotiable Gate)

Every `dcc-mcp-cli` command must conform to a uniform output contract:

**1. Three-channel output format**

| Channel | Flag | Target | Schema |
|---------|------|--------|--------|
| `human` | `--output human` (or default TTY) | Terminal operator | Pretty-printed tables, color, emoji |
| `json` | `--output json` | Agent / script consumer | Single JSON object per invocation |
| `ndjson` | `--output ndjson` | Streaming / log pipeline | One JSON object per line |

- `--output json` is the **default when stdout is not a TTY** (pipe, redirect, CI).
- `--output human` is the **default when stdout is a TTY** (interactive terminal).
- `--output ndjson` is opt-in, used for long-running streaming commands (`wait-ready`, `watch`).
- The three channels share one serialization path per command — no per-command ad-hoc formatting.

**2. stdout/stderr separation**

- **stdout** = machine-readable data (JSON/NDJSON output, or human-readable tables).
- **stderr** = diagnostics, progress bars, warnings, logs.
- Agents parse stdout; humans read stderr. Never mix data into stderr or diagnostics into stdout.
- When `--output json` or `--output ndjson`, all diagnostic output goes to stderr exclusively.

**3. `--non-interactive` mode**

- When `--non-interactive` (or `DCC_MCP_NON_INTERACTIVE=true`), the CLI **must never**:
  - Prompt for input (stdin read without `--json-file -` is a prompt).
  - Wait for confirmation (Y/n, "press enter to continue").
  - Fall back to interactive TTY when JSON input is missing.
- If required input is missing in non-interactive mode, fail immediately with exit code 2.
- `--execute` (install) in non-interactive mode skips consent prompt but still requires
  `DCC_MCP_INSTALL_DISABLED` is not set.

**4. Semantic exit codes**

| Code | Meaning | When |
|------|---------|------|
| 0 | Success | Command completed as requested |
| 1 | General error | Unexpected failure, see stderr |
| 2 | Invalid input | Missing required argument, invalid JSON, schema violation |
| 3 | Unavailable | Gateway not running, DCC not found, endpoint unreachable |
| 4 | Timeout | Operation exceeded `--timeout-secs` |
| 5 | Cancelled | SIGINT/SIGTERM received, partial work may exist |
| 6 | Permission denied | Auth failure, access denied, `--non-interactive` prompt blocked |
| 7 | Conflict | Instance already exists, version conflict, state conflict |

Exit codes are stable — a script written against code 3 will not break when new error
cases are added. New error conditions map to existing codes or add a new code ≥8.

**5. Timeout/cancellation semantics**

- Every network-bound command accepts `--timeout-secs` (default varies by command).
- Timeout → exit code 4, JSON error envelope on stdout, diagnostic on stderr.
- Cancellation (SIGINT/SIGTERM) → exit code 5. If partial work was committed, the
  JSON response includes `"cancelled": true` and `"partial_result": {...}`.

**6. Unified error envelope**

All errors (stdout) follow a single schema:

```json
{
  "error": {
    "code": "UNAVAILABLE",
    "message": "Gateway not responding on 127.0.0.1:9765",
    "exit_code": 3,
    "retryable": true,
    "details": {
      "gateway_host": "127.0.0.1",
      "gateway_port": 9765,
      "last_health_check": "2026-07-25T10:00:00Z"
    }
  }
}
```

- `error.code` is a stable machine-readable snake_case identifier (not the exit code number).
- `error.retryable` is a boolean.
- `error.details` is command-specific context, always an object.
- On success, `error` is absent; presence of `error` alone signals failure.

### P0 — Instance State Actionability

**1. Unified state vocabulary**

Every instance row (from `list`, `doctor`, gateway inventory) exposes:

```json
{
  "instance_id": "abc12345",
  "dcc_type": "maya",
  "status": "available",
  "dispatch_status": "ready",
  "retryable": false,
  "recommended_next_action": "route_call",
  "diagnostics": {}
}
```

**State machine:**

```
booting → available ──→ busy ──→ available
  │          │            │
  └──→ unavailable    └──→ unavailable
```

| `status` | Meaning | `dispatch_status` | `retryable` |
|----------|---------|-------------------|-------------|
| `booting` | Instance starting, not yet accepting calls | `pending` | `true` |
| `available` | Accepting calls, no active session | `ready` | `false` |
| `busy` | Accepting calls, active session in progress | `ready` | `false` |
| `unavailable` | Not accepting calls (crashed, stopped, unreachable) | `failed` or `unknown` | depends on failure |

`dispatch_status` is orthogonal to `status`:
- `ready` = dispatch path verified (host RPC bridge healthy, skill catalog loaded).
- `pending` = dispatch path not yet verified (sidecar starting, bridge connecting).
- `failed` = dispatch path verification failed (host RPC error, bridge crash).
- `unknown` = dispatch status not reported (legacy instance, pre-dispatch version).

**2. `recommended_next_action` per state**

| State | `dispatch_status` | `recommended_next_action` |
|-------|-------------------|---------------------------|
| `booting` | `pending` | `wait_ready` |
| `available` | `ready` | `route_call` |
| `busy` | `ready` | `wait_idle` or `route_call` |
| `unavailable` | `failed` | `diagnose` |
| `unavailable` | `unknown` | `check_instance` |

**3. Discovery health ≠ dispatch availability**

- `list` shows **discovery health** (is the instance registered and heartbeating?).
- `doctor` shows **dispatch availability** (can the instance actually execute tools?).
- `call` only routes to instances where `dispatch_status=ready`.
- `list` rows include both pieces of information so agents don't need to cross-reference.

### P1 — Task-Oriented Skills & Progressive Disclosure

**1. Skills organized by user task, not by underlying tool**

Current organization: one skill per adapter tool. Proposed organization:

| Task Domain | Skill Name | What it covers |
|-------------|-----------|----------------|
| Verify | `dcc-verify` | Health check, smoke test, capability listing |
| Debug | `dcc-debug` | Scene inspection, selection query, log retrieval |
| Build | `dcc-build` | Scene construction, asset import, material setup |
| UI | `dcc-ui` | Viewport control, panel management, screenshot |
| Asset | `dcc-asset` | Export, import, format conversion, USD pipeline |

Each domain skill exposes 3–8 tools that form a coherent workflow. The agent
loads one skill and completes the task, rather than loading 5 atomic skills.

**2. Progressive disclosure via `references/`**

Every domain skill ships with:
- `references/RECIPES.md` — ~20 copy-pasteable task recipes.
- `references/INTROSPECTION.md` — how to query live DCC state.
- `references/ERRORS.md` — common errors and recovery steps.

References are loaded on-demand by the agent, not pre-loaded into the prompt.
This follows the thin-harness pattern from ADR 003.

**3. Backward compatibility**

Existing atomic skills (per-tool wrappers) are not removed. The task-oriented
skills are additive, and agents can fall back to atomic skills for edge cases.
The `search`/`describe`/`load-skill` flow still works for both.

### P1 — Destructive Action Policy

**1. Three-tier action classification**

Every tool call is classified at registration time:

| Tier | Label | Examples | Default Behavior |
|------|-------|----------|-----------------|
| `read` | No side effects | `list`, `search`, `describe`, `get_*` | Always allowed |
| `write` | Reversible side effects | `create_*`, `set_*`, `import_*` | Allowed with audit |
| `destructive` | Irreversible side effects | `delete_*`, `clear_*`, `uninstall`, `stop_instance` | Requires explicit confirmation |

**2. Audit trail**

Every `write` and `destructive` call carries:
- `session_id` — agent session or task identifier.
- `request_id` — unique per-call UUID.
- `timestamp` — ISO 8601 with timezone.

These are written to the gateway audit log and exposed via `stats --session-id`.

**3. Safety gates**

- Destructive actions in `--non-interactive` mode require an explicit `--force` flag.
- Destructive actions without `--force` in non-interactive mode fail with exit code 6.
- `stop-instance` requires `--expected-owner` in non-interactive mode.
- Timeout and cancellation apply uniformly to all three tiers.

**4. Principle**

Dangerous operations are not default-open just because "AI convenience" requests it.
Every destructive path must have an explicit, auditable gate that an agent must
consciously pass through — not a prompt it can auto-confirm.

### P2 — Structured Snapshots & Diff

**1. Structured state capture**

Beyond the existing `dcc-mcp-server capture` (traffic-level recording), the CLI
should support **semantic state snapshots**:

```bash
dcc-mcp-cli snapshot scene --dcc-type maya --instance-id abc12345
dcc-mcp-cli snapshot selection --dcc-type blender --instance-id def67890
dcc-mcp-cli snapshot render-settings --dcc-type houdini --instance-id ghi11223
```

Each snapshot produces a typed JSON document:

```json
{
  "snapshot_type": "scene_graph",
  "dcc_type": "maya",
  "instance_id": "abc12345",
  "timestamp": "2026-07-25T10:00:00Z",
  "payload": {
    "nodes": 1423,
    "root_objects": ["persp", "top", "front", "side"],
    "selection": ["pSphere1"]
  }
}
```

**2. Semantic diff**

```bash
dcc-mcp-cli snapshot diff before.json after.json --type scene_graph
```

Diffs are semantic (e.g. "3 nodes added, 1 material changed") rather than raw
JSON text diff. The agent gets a structured summary it can verify.

**3. P2 status**

This is deferred to a future milestone (tracked via PIP-2881 child issue).
The snapshot schema is designed to be extensible: DCC adapters register
snapshot providers via the existing skill mechanism.

---

## Rationale

| Decision | Why |
|----------|-----|
| Unified output contract | Agents currently must parse per-command output formats. A single contract eliminates per-command special-casing. |
| stdout/stderr separation | Standard Unix contract; every CLI tool that mixes data into stderr creates parsing fragility. |
| `--non-interactive` mode | Without this, agent-driven CLI use is brittle — prompts hang forever or get garbage stdin. |
| Semantic exit codes | `anyhow::Error` currently maps everything to exit code 1. Scripts need to distinguish "not found" (retry) from "invalid input" (fix the request). |
| Instance state vocabulary | Current state labels are inconsistent across `list`, `doctor`, and gateway inventory. A single vocabulary means agents write one state machine. |
| Task-oriented skills | Loading 5 skills for one task inflates context. One skill with progressive disclosure is cheaper and more reliable. |
| Destructive action tiers | The current tool surface doesn't distinguish read/write/destructive. Agents should not accidentally delete scene contents. |
| P2 deferred | Snapshots require adapter-side work (each DCC has different scene graph access). The schema is designed first; implementation follows. |

---

## Consequences

- **New `--output ndjson` mode** added to `OutputFormat` enum in CLI.
- **New `--non-interactive` global flag** and `DCC_MCP_NON_INTERACTIVE` env var.
- **New `--timeout-secs` flag** standardized across all network commands.
- **New `ExitCode` enum** in `dcc-mcp-cli` mapping to process exit codes 0–7.
- **New `ErrorEnvelope` struct** replacing ad-hoc `eprintln!` error output.
- **Instance state vocabulary** formalized in `dcc-mcp-models` — `list`/`doctor`/gateway inventory all emit the same `InstanceStatus` schema.
- **Task-oriented skills** created as new skill packages (not replacing existing atomic skills).
- **Action tier metadata** added to tool registration — `dcc-mcp-actions` and gateway `tools/list` include `action_tier` field.
- **`--force` flag** required for destructive actions in non-interactive mode.
- **Snapshot provider trait** defined in `dcc-mcp-models` for P2 (deferred implementation).
- **Existing commands continue to work** — the contract is additive. `--output pretty` maps to human TTY, `--output json` stays the default for pipes.

---

## Related ADRs

- ADR 003 — Thin-Harness Skill Authoring Pattern (task-oriented skills build on this)
- ADR 002 — DCC Main-Thread Affinity (destructive action policy must respect main-thread constraints)
- ADR 012 — OS-assigned Ports (instance state vocabulary must include port assignment)
- ADR 013 — Persistent Tool-Call Analytics (destructive action audit feeds into this)
- ADR 017 — Codex-style Record & Replay (structured snapshots are the semantic complement)
