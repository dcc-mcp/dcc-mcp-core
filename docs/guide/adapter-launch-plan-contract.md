# Adapter Launch Plan and Readiness Contract

This is the adapter-author contract behind `dcc-mcp-cli start-instance`.
Read it if you maintain a DCC adapter and want agents to be able to go from
"no instance is running" to "a typed session is ready" without a human opening
the host by hand.

The division of responsibility is fixed and is the whole point of the design:

| Layer | Owner | Contents |
|-------|-------|----------|
| Lifecycle | Core | Authorization, project binding, reuse, spawn, registration wait, readiness wait, timeout, guarded stop |
| Readiness vocabulary | Core | The readiness bits and the blocking-state taxonomy |
| Launch plan | **Adapter** | Which executable to run, with which argv, for which project |
| Runtime metadata | **Adapter** | Operation ID, project binding, window handle, host progress, blocking state |
| Readiness values | **Adapter** | What `/v1/readyz` reports for this host |

Core never invents a DCC-specific command line. With no launch plan the
operation fails closed with `blocking_state: launch_plan_missing` instead of
guessing an executable, and it never downloads, installs, upgrades, or edits
configuration.

Command-level usage lives in [cli-reference.md](cli-reference.md). This
document covers what **your adapter** has to publish.

## The zero-instance gap

`dcc-mcp-cli list` reports `total: 0` when no host process is running, even for
a DCC whose adapter is installed and receipted against an exact project. The
agent then has no structured way forward.

`dcc-mcp-cli dcc-types --dcc-type <dcc> --project <path>` now recommends
`start-instance` in exactly that state — but only when a **validated launch
plan resolves** for that DCC type and project. Until your adapter publishes one,
the recommendation falls back to `install`, and the gap stays open.

## Step 1 — Publish a launch plan

### Where Core looks

Resolution is deterministic and the most specific source wins. The first hit is
used; later sources are not consulted.

| Order | Location | Reported `launch_plan_source` |
|-------|----------|-------------------------------|
| 1 | `--launch-plan <path>` on the command line | `explicit` |
| 2 | `<project>/.dcc-mcp/launch-plan.json` | `project_receipt` |
| 3 | `<registry_dir>/launch-plans/<dcc_type>.json` | `state_registry` |

`<registry_dir>` is `$DCC_MCP_REGISTRY_DIR`, falling back to
`<temp>/dcc-mcp-registry`. The state-registry file name is the DCC type
normalized to lowercase with spaces and dashes replaced by underscores, so
`3ds Max` becomes `3ds_max.json`.

Publish per project (source 2) when the executable depends on the project —
a Unity project pinned to one Editor version, a Blender file tied to one
build. Publish per machine (source 3) when one install serves every project.
Both may exist; the project receipt wins.

### Schema v1

```json
{
  "schema_version": 1,
  "dcc_type": "unity",
  "executable": "C:/Program Files/Unity/Hub/Editor/2022.3.10f1/Editor/Unity.exe",
  "argv": ["{executable}", "-projectPath", "{project}", "-noUpmPrefetch"],
  "version": "2022.3.10f1",
  "project_markers": ["ProjectSettings/ProjectVersion.txt"],
  "cwd": null
}
```

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `schema_version` | integer | no | Defaults to `1`. A value **greater** than `1` is rejected |
| `dcc_type` | string | yes | Must equal the requested `--dcc-type`, compared case-insensitively |
| `executable` | string | yes | Absolute path that must exist **as a file** at launch time |
| `argv` | array of strings | yes | Non-empty. `argv[0]` must expand to exactly `executable` |
| `version` | string | no | Host version. Compared as an exact string when `--version` is passed |
| `project_markers` | array of strings | no | Relative paths that must exist inside the project |
| `cwd` | string | no | Working directory for the spawned host. Falls back to the project root when unset or not a directory |

A second example, this time per-machine and for a different host family:

```json
{
  "schema_version": 1,
  "dcc_type": "blender",
  "executable": "/usr/local/blender/4.2/blender",
  "argv": ["{executable}", "--python-expr", "import dcc_mcp_blender.bootstrap; dcc_mcp_blender.bootstrap.serve()", "{project}"],
  "version": "4.2.1",
  "project_markers": [],
  "cwd": null
}
```

### Validation rules that fail closed

Every rule below terminates the operation with a structured report; none of
them fall back to a guessed command line.

| Rule | `blocking_state` |
|------|------------------|
| `schema_version` greater than the supported version | `invalid_launch_plan` |
| `dcc_type` does not match the requested DCC type | `invalid_launch_plan` |
| Unreadable or malformed JSON | `invalid_launch_plan` |
| `argv` is empty | `invalid_launch_plan` |
| `argv[0]` is not byte-identical to `executable` after expansion | `invalid_launch_plan` |
| `executable` is not absolute, or is not an existing file | `missing_executable` |
| `--version` was passed and does not equal `version` exactly | `version_mismatch` |
| A declared `project_markers` entry is missing on disk | `project_marker_missing` |
| `--project` is not an existing directory | `project_not_found` |
| No plan resolved from any source | `launch_plan_missing` |
| Operator did not pass `--yes` | `authorization_required` |

`project_markers` is how Core refuses to launch the wrong host. For Unity,
`ProjectSettings/ProjectVersion.txt` is the right marker; for a host with no
project file, leave the list empty rather than inventing a weak marker.

## Step 2 — argv placeholders

Core expands these tokens in every argv element before spawning:

| Placeholder | Expands to |
|-------------|------------|
| `{executable}` | The `executable` path, verbatim |
| `{project}` | The canonicalized project root |
| `{dcc_type}` | The normalized DCC type (lowercase) |
| `{version}` | `version` from the plan, or `--version`, or the empty string |

Rules that matter:

- Expansion is a plain string replace, in the order executable, project,
  DCC type, version. There is no shell: tokens are passed to the OS verbatim,
  so you cannot rely on quoting, globbing, or `&&`.
- `argv[0]` after expansion must be **byte-identical** to `executable`. A plan
  that expands `argv[0]` to a sibling binary — even one with the same file
  name in a different directory — is rejected. Core verifies the path it
  recorded, and it will not spawn a different one.
- `{project}` receives the canonicalized absolute path. On Windows the
  `\\?\` prefix is stripped so the value stays operator-readable.
- Because expansion is ordered, a path that itself contains `{project}`,
  `{dcc_type}`, or `{version}` is expanded a second time. Avoid publishing
  executables or projects whose paths contain those literals.

## Step 3 — Environment variables handed to the host

Core spawns the host detached, with stdin/stdout/stderr redirected to null, and
sets three variables so the adapter can correlate the whole lifecycle:

| Variable | Value |
|----------|-------|
| `DCC_MCP_START_OPERATION_ID` | The operation ID, stable from launch through terminal readiness |
| `DCC_MCP_START_PROJECT` | The canonical project root |
| `DCC_MCP_START_DCC_TYPE` | The normalized DCC type |

**Your adapter must copy `DCC_MCP_START_OPERATION_ID` into its registry row.**
It is the reliable way for Core to bind a freshly registered instance back to
the operation that spawned it. Core also falls back to matching the launched
PID, but PIDs get reused after a crash, so the operation ID is the dependable
path and the PID is only the second choice (see Step 7).

`start-instance` does not set `DCC_MCP_LAUNCH_ID`. That field and the `role`
field are the pre-existing identity contract from RFC-0007 §3.2, and they
already give you launch-scoped grouping and host-versus-sidecar
disambiguation. Nothing new is required from you for the "reuse an existing
instance instead of launching another" behaviour.

## Step 4 — Stamp the registry row

Everything in this section is what Core reads back out of the FileRegistry row.
Three mechanisms are available, all on `str -> str` metadata unless stated:

1. `McpHttpConfig.instance_metadata` — set it before `server.start()`. Best for
   values known at startup, such as the operation ID and project path.
2. `handle.update_gateway_metadata({...})` — merge at runtime; an empty string
   clears the key. Best for progress and blocking state.
3. `handle.update_gateway_extras({...})` / `McpHttpConfig.instance_extras` —
   JSON-typed counterpart, for numbers, booleans, and nested values.

`DccServerBase.update_gateway_metadata()` is **not** the right call here: it
only accepts `scene`, `version`, `documents`, and `display_name`. Use
`instance_metadata` on the config or the raw handle for the keys below.

Core looks each key up in `metadata` first, then in `extras`, so either
mechanism works for the string-valued keys.

### Identity and binding

| Purpose | Keys, in lookup order |
|---------|-----------------------|
| Operation binding | `dcc_mcp_operation_id`, `operation_id`, `dcc_mcp.operation_id` |
| Project binding | `dcc_mcp_project`, `project`, `dcc_mcp.project` |

**Both are load-bearing, but neither is strictly required.** Reuse,
convergence, and the guarded stop prefer them. Core matches a live instance to
an owning operation in three independent ways — operation ID, PID, or project
binding — so stamping neither key removes two of the three, not all of them
(see Step 7). What you lose is reliability, not convergence: the remaining PID
path can converge onto an unrelated instance once the OS recycles a PID, and it
matches nothing when the recorded executable is a launcher that spawns the real
host as a separate process, or when a sidecar row omits `host_pid`. Stamp both
keys and none of that applies.

### Native window handle

`window_handle`, `native_window_handle`, `hwnd`, `dcc_mcp_window_handle`,
`dcc_mcp.window_handle`. Optional; surfaced as `window_handle` on the report.
Note that Core does not publish this for you: `DccServerBase` resolves
`dcc_window_handle` for UI Control scoping internally, but that value is not
written to the registry row. Publish it yourself if you want it reported.

### Host progress (advisory)

`compiling`, `importing`, `refreshing`, `updating`, `play_mode`,
`domain_reload`, `progress_stage`, `progress_phase`, `progress_message`.

Core surfaces whichever of these are present, verbatim and uninterpreted, as
`host_progress` on the report. **They are diagnostics only — Core never gates
readiness on them.** Reporting `compiling: "true"` does not extend the timeout
or delay the terminal report; only the `/v1/readyz` bits from Step 5 do. Use
these to explain *why* a host is not ready yet, not to ask for more time.

### Blocking state (see Step 6)

`restart_required` / `dcc_mcp_restart_required` / `dcc_mcp.restart_required`,
`blocking_dialog` / `modal_dialog` / `dcc_mcp_blocking_dialog` /
`dcc_mcp.modal_dialog`, `project_lock` / `project_locked` /
`dcc_mcp_project_lock` / `dcc_mcp.project_lock`, `license_state` /
`license_status` / `dcc_mcp_license_state` / `dcc_mcp.license_state`,
plus `failure_stage` and `failure_reason`.

### Structured diagnostics

`failure_stage`, `failure_reason`, `failure_at_unix`, `host_rpc_uri`,
`host_rpc_scheme`, `sidecar_pid`, `gateway_health_url`,
`gateway_recovery_driver`, `registration_refresh_mode`,
`gateway_guardian_active`, `gateway_guardian_failures`,
`gateway_guardian_restarts`, and the log-path keys (`sidecar_log_dir` /
`stdio_log_dir` / `log_dir`, and the matching `*_stdout_path` /
`*_stderr_path`). These populate `diagnostics` on the report.

### Worked example

```python
import os

from dcc_mcp_core import McpHttpConfig


def build_config(project: str) -> McpHttpConfig:
    config = McpHttpConfig(dcc_type="unity")
    metadata: dict[str, str] = {}

    operation_id = os.environ.get("DCC_MCP_START_OPERATION_ID")
    if operation_id:
        metadata["dcc_mcp_operation_id"] = operation_id

    # Prefer the env var Core injected; fall back to the adapter's own root.
    metadata["dcc_mcp_project"] = os.environ.get("DCC_MCP_START_PROJECT", project)

    config.instance_metadata = metadata
    return config


# Later, once the host is compiling:
handle.update_gateway_metadata({
    "progress_stage": "compiling",
    "compiling": "true",
})

# And when compilation finishes:
handle.update_gateway_metadata({
    "progress_stage": "",
    "compiling": "",
})
```

## Step 5 — Report readiness through `/v1/readyz`

`start-instance --wait-ready` polls the instance's `/v1/readyz` until every
required bit is `true` or the timeout expires.

| Bit | Meaning |
|-----|---------|
| `process` | The HTTP listener answers |
| `dcc` | The host has finished initializing |
| `skill_catalog` | Search/load metadata is usable |
| `dispatcher` | The action dispatcher is wired |
| `host_execution_bridge` | A host execution bridge is attached |
| `main_thread_executor` | The bridge has a running main-thread pump |

Semantics to rely on:

- Default requirement is `process,dcc,skill_catalog,dispatcher`. Pass
  `--require` to change it; values are lowercased with dashes folded to
  underscores.
- A bit counts as satisfied **only when it is boolean `true`**. Any other type,
  including the string `"true"`, is treated as missing.
- `host_execution_bridge` and `main_thread_executor` are deliberately outside
  the default set. Require them explicitly with
  `--require host_execution_bridge,main_thread_executor` when you are
  validating main-thread-only tools.
- If `skill_catalog` is required and `/v1/readyz` reports it `false`, Core
  probes the instance's discovery MCP `tools/list` for `search_tools`,
  `search_skills`, or `load_skill`. If one is present the bit is upgraded to
  `true` with `skill_catalog_source: "discovery_mcp"`. An adapter that exposes
  those tools gets credit even if its readiness payload omits the bit.

## Step 6 — Blocking states

Core classifies your metadata into one terminal state and always attaches a
non-interactive `next_action` with an exact `command` array.

| `blocking_state` | How your adapter triggers it | `retryable` | Safe next step |
|------------------|------------------------------|-------------|----------------|
| `none` | Terminal readiness reached | no | Continue with `search` / `describe` / `call` |
| `restart_required` | `restart_required` truthy, or `failure_stage`/`failure_reason` contains the word `restart` | no | Stop the owned instance, then start again |
| `project_lock` | `project_lock` / `project_locked` truthy, or the words `lock`, `locked` | yes | Close the other host or pick another project, then retry |
| `license` | `license_state` / `license_status` other than `valid`, `ok`, `active`, or the word `license` | no | Resolve the license in the host UI, then retry |
| `modal_dialog` | `blocking_dialog` / `modal_dialog` truthy, or the words `dialog`, `modal` | yes | Dismiss the dialog in the host UI, then retry |
| `adapter_bootstrap` | `failure_stage` / `failure_reason` contains `bootstrap` or `sidecar` | yes | Inspect instance diagnostics, then retry |
| `missing_executable` | Plan `executable` is gone or is not a file | no | Re-run the adapter install so the receipt points at a real executable |
| `version_mismatch` | `--version` does not equal the plan's `version` | no | Start without `--version`, or install the requested version |
| `ambiguous_reuse` | Several live instances claim the same project | **no** | Stop the extra instance or pass `--instance-id` |
| `launch_plan_missing` | No plan resolved | no | Install the adapter so it publishes a validated plan |
| `authorization_required` | `--yes` was not passed | no | Re-run with `--yes` after operator review |
| `timeout` | The operation hit `--timeout-secs` before readiness | yes | Retry with a larger timeout, or inspect diagnostics |
| `cancelled` | The operator cancelled | no | Re-run when the host may be started |
| `invalid_launch_plan` | Plan is malformed or unsafe | no | Re-install the adapter so it republishes a plan |
| `project_not_found` | `--project` is not an existing directory | no | Pass the exact absolute project root |
| `project_marker_missing` | A declared `project_markers` entry is absent | no | Verify the project is the one this adapter was installed for |
| `launch_failed` | Spawning the executable failed | yes | Inspect the executable path and OS error, then retry |

`truthy` in the table above means an exact match against a fixed whitelist,
applied after the value is trimmed and lowercased: `1`, `true`, `yes`,
`blocked`, `blocking`, `present`, `open`. Anything else is false, so
`restart_required: "on"`, `blocking_dialog: "2"`, and `modal_dialog: "visible"`
all fall through to `none` with no warning that the value was not understood.
An empty value, or the literal `none` in any case, is discarded before the
comparison and is likewise treated as absent.

`license_state` / `license_status` are the exception: they are not a truthy
test. Any value other than `valid`, `ok`, or `active`, compared
case-insensitively, classifies as `license` — so `license_state: "on"` does
report a license problem.

Two details worth internalizing:

**`ambiguous_reuse` is deliberately not retryable.** Its recovery step asks the
operator to stop an instance or pass `--instance-id`. Replaying the identical
request reproduces the same ambiguity forever, so reporting it as retryable
would tell an agent to spin on a request that cannot succeed unchanged. Every
other state marked `retryable: true` can be replayed as-is once a human has
cleared the underlying condition.

**Free-text matching is whole-word only, against a fixed keyword list.**
`failure_stage` and `failure_reason` are concatenated, split on
non-alphanumeric characters, lowercased, and matched as complete words — never
as substrings. `blocked` and `unlocked` do **not** classify as a project lock,
and `lockfile` does not either. `sidecar_bootstrap` does match, because it
splits into `sidecar` and `bootstrap`.

The complete list, evaluated in this order — the first group that hits wins:

| Order | Keywords | Classifies as |
|-------|----------|---------------|
| 1 | `bootstrap`, `sidecar` | `adapter_bootstrap` |
| 2 | `license`, `licence` | `license` |
| 3 | `dialog`, `modal` | `modal_dialog` |
| 4 | `lock`, `locks`, `locked`, `locking` | `project_lock` |
| 5 | `restart`, `restarts`, `restarting` | `restart_required` |

Words outside these five groups are ignored, so a diagnostic reading
"compilation is blocked" classifies as `none`. Name the condition using one of
the exact words above.

Structured keys are interpreted first, and only then free text. Prefer the
structured key — `license_state: "expired"` beats a prose `failure_reason`.

## Step 7 — Convergence and the guarded stop

Core persists one record per operation under
`<registry_dir>/start-instance/<operation_id>.json`, with an `index.json`
mapping `dcc_type|canonical-project` to the owning operation ID.

Convergence works like this: a live instance is reusable when it advertises the
owning operation ID, or its PID matches the owning operation's PID, or its
project binding matches the requested project. When several instances survive
that filter, routable ones win; more than one still remaining is
`ambiguous_reuse`.

Stop is separate and guarded. `stop-instance --operation-id <id>` refuses
unless all of the following hold:

- the operation record exists locally;
- the operation `owned` the process — it launched it itself;
- the operation registered an instance ID;
- the record's DCC type matches, and the target instance matches the owned one.

An operation that only converged onto someone else's running host is not owned
and cannot stop it. That is intentional: `start-instance` only ever stops what
it started.

## Operator surface

```bash
# Resolve and report the plan without spawning anything.
dcc-mcp-cli start-instance --dcc-type unity --project /abs/path/MyProject --dry-run

# Launch and wait for terminal readiness.
dcc-mcp-cli start-instance --dcc-type unity --project /abs/path/MyProject --wait-ready --yes

# Narrow a readiness gate, and give a slow host more time.
dcc-mcp-cli start-instance --dcc-type unity --project /abs/path/MyProject \
  --wait-ready --require skill_catalog,host_execution_bridge --timeout-secs 600 --yes

# Disambiguate several instances claiming the same project.
dcc-mcp-cli start-instance --dcc-type unity --project /abs/path/MyProject \
  --instance-id 3f2a1c9e --wait-ready --yes

# Stop only what the operation launched and owns.
dcc-mcp-cli stop-instance --operation-id <operation-id>
```

| Flag | Effect |
|------|--------|
| `--dcc-type <dcc>` | Target DCC type; matched case-insensitively against the plan |
| `--project <path>` | Absolute project root; must exist and is canonicalized |
| `--launch-plan <path>` | Explicit plan document, overriding both other sources |
| `--version <v>` | Pin the plan's `version` by exact string equality |
| `--instance-id <id>` | Converge on one exact instance instead of any project match |
| `--wait-ready` | Wait for terminal readiness instead of stopping at registration |
| `--require <bits>` | Comma-separated readiness bits; defaults to `process,dcc,skill_catalog,dispatcher` |
| `--timeout-secs <n>` | Overall budget; defaults to `300` |
| `--interval-secs <n>` | Poll interval; defaults to `1` |
| `--dry-run` | Resolve and report the plan without spawning |
| `--yes` | Operator authorization to launch a GUI process |

Exit codes: a report with `ok: false` exits `Unavailable`; `--wait-ready` that
returns `ready: false` exits `Timeout`. A `--dry-run` reports `ready: false` by
design and still exits successfully — a dry run is never a timeout.

Without `--wait-ready` the report stops at `stage: registration` and never
claims `terminal`: **process creation is not adapter readiness.**

## Adapter acceptance checklist

- [ ] Install writes `.dcc-mcp/launch-plan.json` into the project, or
      `<registry_dir>/launch-plans/<dcc_type>.json` for the machine.
- [ ] `executable` is absolute and exists as a file at launch time.
- [ ] `argv` is non-empty and `argv[0]` expands to exactly `executable`.
- [ ] `project_markers` names a file that only this DCC's projects carry, or is
      empty.
- [ ] The host copies `DCC_MCP_START_OPERATION_ID` into
      `dcc_mcp_operation_id` on its registry row.
- [ ] The host publishes `dcc_mcp_project` on its registry row.
- [ ] `/v1/readyz` reports `process`, `dcc`, `skill_catalog`, and `dispatcher`
      as booleans, and only flips them to `true` when they are genuinely true.
- [ ] Long-running startup states are reported through the host-progress keys,
      with the understanding that they are advisory only.
- [ ] Blocking conditions are published with the structured key rather than
      prose `failure_reason`, and use whole words.
- [ ] `dcc-mcp-cli start-instance --dry-run` resolves the plan for a real
      project.
- [ ] A second identical `start-instance` returns `reused: true` and
      `converged_on_operation_id` instead of launching a second host.

## Sharp edges and known gaps

These are properties of the shipped contract, not bugs to work around silently.
They are recorded here so adapter authors do not rediscover them:

1. **No Python helper writes the plan.** Adapters write `launch-plan.json`
   directly. There is no `dcc_mcp_core` API that emits or validates the
   document, so a schema typo is only caught at `start-instance` time as
   `invalid_launch_plan`.
2. **`DccServerBase.update_gateway_metadata()` cannot stamp these keys.** It
   accepts only `scene`, `version`, `documents`, and `display_name`. Use
   `McpHttpConfig.instance_metadata` at startup or
   `handle.update_gateway_metadata()` at runtime.
3. **`window_handle` is not published for you.** Core resolves a window handle
   for UI Control scoping internally but does not write it to the registry row.
   Adapters that want `window_handle` on the `start-instance` report must
   publish it.
4. **Version pinning is exact string equality.** `--version 2022.3` does not
   match a plan declaring `2022.3.10f1`, and there is no range or semver
   support.
5. **Placeholder expansion is ordered string replacement.** A path containing
   `{project}`, `{dcc_type}`, or `{version}` is expanded twice. Avoid such
   paths.
6. **Host-progress keys are never gated on.** Reporting `compiling: "true"`
   neither extends the timeout nor delays the terminal report.
7. **Convergence degrades to PID matching when neither operation ID nor project
   is stamped.** It does not fail outright: Core still matches the launched PID
   against `pid` or `host_pid` on the registry row. The degraded path is
   unreliable — a recycled PID can converge onto an unrelated instance, and it
   matches nothing when the recorded executable is a launcher that spawns the
   real host as a separate process, or when a sidecar row omits `host_pid`.
   Stamp both keys.
