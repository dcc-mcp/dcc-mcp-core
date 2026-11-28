---
name: dcc-cua
description: >-
  Project-owned DCC-CUA routing skill for bounded application UI automation.
  Use this skill whenever the user says dcc-cua, DCC CUA, our dcc-cua, or
  我们的 dcc-cua, including browser and other non-DCC UI tasks. These phrases
  are a hard route to the dcc-mcp project UI Control stack and take precedence
  over generic Codex/OpenAI Computer Use, the computer-use Skill, @oai/sky,
  and Browser/Chrome plugins. If DCC-CUA is unavailable, repair or report the
  project route; never silently fall back to a generic computer-use provider.
license: MIT-0
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: python
    recall-context:
      app_type: python
      domain: ui-automation
      workflow_stage: automation
      task_category: mutate
    layer: infrastructure
    compatibility: Cross-platform routing contract. The current DCC-CUA host is a separately released runtime forwarded by dccmcp; exact platform capabilities remain runtime-discovered.
    version: "0.1.0"
    search-hint: "dcc-cua DCC CUA dcc cua our dcc-cua our dcc cua 我们的 dcc-cua 我们的 dcc cua project-owned UI control browser DOM exact PID HWND computer use automation"
    tags: "dcc-cua, dcc-ui-control, ui-control, browser-dom, exact-window, computer-use, infrastructure"
  openclaw:
    emoji: "🖱️"
    homepage: https://github.com/dcc-mcp/dcc-mcp-core/tree/main/skills/dcc-cua
---

# DCC-CUA — Project UI Control Router

Use this Skill as the canonical route whenever the user explicitly names
`dcc-cua`, `DCC CUA`, `our dcc-cua`, or `我们的 dcc-cua`.

## Non-substitution contract

An explicit DCC-CUA request is a hard routing boundary, regardless of whether
the target is Maya, Chrome, a browser, or another desktop application.

- Use the project-owned `dcc-cua` runtime and DCC-MCP `ui-control` surface.
- Never load or call generic Codex/OpenAI Computer Use, the `computer-use`
  Skill, `@oai/sky`, or Browser/Chrome automation plugins for that request.
- Never treat a DCC-CUA runtime, binding, readiness, or permission failure as
  permission to change providers.
- Repair the project route when safely possible. Otherwise report the exact
  blocker and stop.
- Use a generic provider only after the user explicitly retracts the DCC-CUA
  requirement or explicitly requests that provider by name.

This boundary is provider selection, not an authorization bypass. DCC-CUA task
grants, target binding, interruption, and confirmation policy still apply.

Embedding developers may use Core's constructor-only `managed_launcher`
dependency only for an exclusively transferred local MCP child. Read
[owned worker launch](../../docs/guide/ui-control-managed-worker.md) before
integration. The embedding must supply the actual authorized launcher; this
preparatory interface does not establish independent supervisor readiness,
attach to a shared worker, export a lease, or permit changing Job policy.

## Automatic route from search

The contract above also covers requests that never name `dcc-cua`. When a
skill or tool search returns no usable interface, the search surface attaches
`fallback` advice naming this Skill, with a reason code:

| reason | meaning |
|---|---|
| `no_candidate` | nothing in the catalog matched the request |
| `low_confidence` | the closest matches are lexical near-misses, not a real interface |
| `no_executable_interface` | the closest match is documentation-only and declares no callable tool |

The advice is **additive**: the hits are still returned, and the routing
criteria never reorder or drop them.

The advice is blocked, with reason code `cua_runtime_unavailable`, when this
runtime is not usable. A blocked response is a blocker to report or repair —
it is not permission to switch provider. The non-substitution contract above
applies unchanged to the automatic route.

Ranking is deliberately not involved. This Skill declares
`layer: infrastructure`, which the search rank policy demotes to `0.35` so it
stays out of neutral discovery. That must not be "fixed" by raising the
coefficient: the route is explicit, not a ranking effect.

## Runtime preflight

Use the standalone DCC-CUA contract; do not download an arbitrary executable:

```bash
dccmcp cua -- manifest
dccmcp cua -- ping
dccmcp cua -- host-ensure
```

Run `dccmcp cua -- host-ensure` only after the standalone `dcc-cua` runtime has
been installed and the operation is authorized. The command forwards to the
independent runtime and does not embed its SDK or own the Host lifecycle.

The two runtimes come from different releases. `dccmcp` is released from
`https://github.com/dcc-mcp/dcc-mcp-cli`, while `dcc-cua` is released from
`https://github.com/dcc-mcp/dcc-cua`. `dccmcp cua -- ...` forwards to the
installed `dcc-cua` runtime and does not download it as part of a CLI update.

If the runtime is missing, install the official `dcc-cua` release separately
from `https://github.com/dcc-mcp/dcc-cua/releases`, then run
`dccmcp cua -- manifest` and `dccmcp cua -- ping` before using UI Control.

Retryable rename/delete failures retain prepared ownership only when a live
OS file identity still matches. Windows requires the OS and filesystem to
support [`FileIdInfo` / 128-bit file IDs](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info);
availability of the older basic handle-information API alone is insufficient.
An unsupported or unreadable initial identity check preserves the ownership
ticket without attempting consumption. If identity cannot be rechecked after
a failed operation, the ticket remains relinquished and inspection is required.
Do not assume compatibility with every older Windows version or filesystem,
or delete an ambiguous sidecar to force recovery.
These checks complement the installation lock, not a filesystem sandbox
against unrelated writers that ignore that lock.

For semantic application profiles:

```bash
dcc-cua profiles
dcc-cua profile --id <profile-id>
```

Do not invent a profile ID. Runtime-advertised capabilities are authoritative.

## Execution order

Before batching keyboard input, inspect the connected DCC-CUA Host's
`guarded_key_sequence_v1` capability. In a Windows foreground pixel task
with the separate `raw_input/key_sequence` grant, pass ordered taps and
chords to one `execute_action`. A visible execution frame and a fresh
same-task observation are required. Keep the normal stop boundary when the
next operation depends on a new selection or dialog.

Reuse the returned post-action observation when continuing visually. For a
terminal edit, an independent scene readback can verify mesh dimensions,
bone parenting or animation keys. Record verified wall time, native dispatch
and inter-step waits separately. Use the platform's advertised observation
and input routes when the batch capability is absent.

1. Prefer a typed DCC-MCP host tool when it directly expresses the operation.
2. Use DCC-CUA only for the UI behavior that typed host tools cannot expose.
3. Bind one exact target with process ID and native window handle whenever the
   surface supports them.
4. Open one scoped session with the minimum task grant required for the work.
5. Take a fresh observation before each action that depends on UI state.
6. Act using stable semantic control or DOM references when available.
7. Wait for a typed state transition and verify the real final state.
8. Stop the session on success, failure, interruption, or abandonment.

An `input sent` acknowledgement is not completion evidence.
For native application menu bars, prefer the negotiated `native_menu_path`
route through `ui_control__act(action="invoke_menu", menu_path=[...])` when a
semantic menu click or Alt mnemonic cannot prove that a popup opened. A menu
invocation invalidates the current observation; honor `verification_required`
and verify the popup or resulting application state with a fresh snapshot.

## DCC-host route

For a registered DCC instance, use the DCC-MCP UI Control tools or their CLI
projection:

```bash
dccmcp load-skill ui-control --instance-id <instance-id> --output toon
dccmcp ui-control snapshot --instance-id <instance-id> --json '{"session_id":"ui","process_id":1234,"window_handle":5678}'
dccmcp ui-control act --instance-id <instance-id> --json '{"session_id":"ui","control_id":"ok","action":"click","snapshot_id":"<snapshot-id>"}'
dccmcp ui-control act --instance-id <instance-id> --json '{"session_id":"ui","action":"invoke_menu","menu_path":["Window","Arrange","Left"]}'
dccmcp ui-control stop --instance-id <instance-id> --json '{"session_id":"ui"}'
```

Use the same exact instance and session throughout the action chain. Do not
switch to another DCC process because it looks similar.

## Browser and non-DCC route

Browser work remains inside DCC-CUA. Use the Host's typed browser surface and
`browser_dom` capabilities, not an in-app Browser or Chrome plugin.

- Bind the exact browser PID and window handle first.
- Bind the exact tab/target returned by DCC-CUA; do not infer it from a title
  alone when an exact target identifier exists.
- Keep connection-scoped sessions and capabilities on one Host connection.
- Use DOM/semantic references from the latest observation rather than stale
  coordinates.
- `browser_prepare` and existing-profile attachment require both the Host grant
  and the session task grant advertised by the runtime contract.
- Authentication challenges, CAPTCHAs, purchases, account/security changes,
  and unexpected permission prompts remain trusted human boundaries.

## Target and evidence invariants

- Preserve PID and native window handle in observations and audit records.
- Treat a changed PID, window handle, tab target, or session owner as a fresh
  binding that requires a fresh observation.
- Keep `full` readiness strict. If only an exact-window or typed-browser route
  is independently ready, report that route-specific degraded readiness.
- Honor Escape/user interruption immediately and do not resume without fresh
  authorization and observation.
- Do not expose local usernames, internal package paths, browser profile data,
  credentials, tokens, or unrelated window titles in public evidence.
- Verify success by reading the destination state after the mutation.

## Failure behavior

When DCC-CUA cannot complete the request, report:

1. the exact component/runtime version,
2. the exact target identity that was bound,
3. the failing readiness, capability, permission, or action stage,
4. the last safe observation or typed error, and
5. the safe next repair step.

Do not mention a generic Computer Use fallback unless the user asks for one.
