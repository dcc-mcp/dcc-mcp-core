# Adapter Install SOP v1

This guide defines the agent-first installation contract for every public
DCC-MCP adapter. It standardizes the interface an agent can discover, execute,
verify, repair, upgrade, and uninstall without guessing adapter-specific
commands or interpreting prose.

The words **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are normative. The
contract applies to adapter-owned installers now and to the shared Core
installer as its remaining execution slices adopt the same behavior. The Core
CLI now emits this schema for plan execution, including real step and rollback
outcomes. Interpreter discovery, receipt ownership, a standalone verification
command, uninstall, idempotent retry, and catalog migration remain separate
implementation slices.

See [Adapter Install Lifecycle](adapter-install-lifecycle.md) for Core's
import-light sidecar, readiness, process, and lock-classification primitives.

## Universal command surface

The universal front door is plan-first:

```bash
dcc-mcp-cli install --dcc-type <dcc>
dcc-mcp-cli install --dcc-type <dcc> --execute --json
```

An adapter that owns host-specific installation behavior MUST expose one
console entry point with these verbs:

```text
dcc-mcp-<dcc> install|status|verify|uninstall|upgrade
```

Every applicable verb MUST use the same flags:

| Flag | Contract |
|---|---|
| `--json` | Emit exactly one JSON result document to stdout. Diagnostics go to stderr. |
| `--yes` | Execute a mutating adapter command without an interactive prompt. |
| `--dry-run` | Resolve and validate the full plan without changing host, package, or receipt state. |
| `--dcc-path <path>` | Select the exact host executable/application and its matching versioned profile. |
| `--python <path>` | Select the exact target interpreter used for package and import checks. |

The Core front door uses `--execute`; adapter-owned commands use `--yes`.
Commands MUST NOT invent another executable name, positional-only project
shape, or adapter-specific spelling for these concepts. Existing legacy entry
points MAY remain as compatibility shims, but documentation and machine-readable
next steps MUST use the standard surface.

`status` and `verify` are read-only. `install`, `upgrade`, and `uninstall` MUST
plan by default and mutate only with their execution flag. `--json` MUST never
cause an interactive prompt.

## Plan and execution contract

The default invocation returns an auditable plan. Planning MUST perform every
safe resolution and preflight needed to predict execution:

- adapter and Core versions;
- resolved DCC executable, DCC version, and versioned user/project profile;
- resolved target interpreter and how it was selected;
- current state: `fresh`, `current`, `upgrade`, `repair`, or `partial`;
- permissions and loaded/locked install-root evidence;
- ordered acquisition, package, host-enablement, receipt, and verification
  steps;
- the stable receipt path; and
- executable recovery steps for anything the installer cannot complete.

`--dry-run` MUST take the same resolution and preflight path as execution and
MUST NOT write staging directories, packages, host configuration, or receipts.
Executing a previously displayed plan MUST repeat safety-sensitive preflight;
filesystem, host, process, or version state may have changed.

The stable process exit codes are exported from `dcc_mcp_core.deployment`:

| Code | Public constant | Meaning |
|---:|---|---|
| `0` | `INSTALL_EXIT_OK` | Plan or operation completed with the reported usable/expected state. |
| `10` | `INSTALL_EXIT_PREFLIGHT` | Host, interpreter, version, permission, policy, or partial-state preflight failed. |
| `20` | `INSTALL_EXIT_ACQUIRE` | A pinned package or artifact could not be acquired or verified. |
| `30` | `INSTALL_EXIT_INSTALL` | Staging, host enablement, receipt commit, uninstall, or rollback failed. |
| `40` | `INSTALL_EXIT_VERIFY` | Artifacts were installed, but verify-to-usable failed. |
| `50` | `INSTALL_EXIT_REQUIRES_RESTART` | A loaded/locked artifact requires deferred cleanup after the host restarts. |

Exit `50` is reserved for real lock/deferred-cleanup evidence. A closed host,
missing readiness probe, or ordinary verification failure is exit `40`, not a
fabricated restart requirement.

## JSON result contract

Every verb MUST emit Install SOP schema version 1 with these required fields:

```json
{
  "schema_version": 1,
  "status": "planned",
  "dcc_type": "example",
  "adapter_version": "1.2.3",
  "core_version": "0.20.9",
  "steps": [
    {"id": "preflight", "status": "ok"},
    {"id": "install", "status": "planned"},
    {"id": "verify", "status": "planned"}
  ],
  "next_steps": [
    {
      "id": "execute",
      "description": "Execute the validated install plan.",
      "command": ["dcc-mcp-example", "install", "--json", "--yes"],
      "why": "Planning does not mutate the host."
    }
  ],
  "receipt_path": null,
  "verify": {
    "directly_usable": false,
    "failure_stage": null,
    "failure_reason": null
  }
}
```

Load a fresh schema document without importing the native extension:

```python
from dcc_mcp_core.deployment import load_install_sop_schema

schema = load_install_sop_schema()
```

The same document is packaged as
`dcc_mcp_core/schemas/adapter-install-sop-v1.schema.json` with canonical id
`https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json`.

The required top-level fields are stable for schema v1. Additive adapter fields
are allowed. A consumer MUST reject an unsupported higher schema version with
an upgrade diagnostic; it MUST NOT silently reinterpret, delete, or rewrite the
result.

For the Core CLI, `install --dcc-type <dcc> --json` remains a non-mutating plan
with its existing plan shape. `install --dcc-type <dcc> --execute --json`
instead emits exactly one post-execution schema-v1 document and uses the stable
Install SOP process exit code reported in `exit_code`. It never echoes the
pre-execution plan as an execution result. The report includes stable step IDs,
observed step states, rollback attempts and outcomes, executable recovery
commands, nullable receipt state, and verification state. Raw paths, subprocess
output, exception text, and credentials do not enter public fields or operator
diagnostics. `--execute` is the explicit mutation opt-in, so JSON execution does
not add an interactive prompt.

The current Core executor verifies local install artifacts but does not launch
or control the DCC. A locally successful execution therefore reports the
install steps as `ok` while keeping `verify.directly_usable` false with
`LIVE_DCC_VERIFICATION_REQUIRED`; `confirm-readiness` is returned as an
executable next step. This is a truthful boundary, not a substitute for the
later standalone verification slice.

`status` is one of `planned`, `running`, `ok`, `failed`, `partial`, or
`requires_restart`. Each `steps[]` entry has a stable `id` and `status`.
Additional duration, result, rollback, or diagnostic fields MAY be attached to
the step.

Each `next_steps[]` entry MUST contain `id`, `description`, and `why`, plus
exactly one executable form:

- `command`: an argv array of non-empty, non-whitespace arguments, never a
  shell-joined string; or
- `file_edit`: `{path, action, content?}` with `action` equal to `create`,
  `update`, or `remove`. `content` is required for `create` and `update`, and
  MUST be omitted for `remove`.

Prose-only next steps are non-conforming. Paths, arguments, and edits MUST be
specific enough for an agent to execute after applying its normal policy and
confirmation boundaries. Credentials and secrets MUST NOT appear in results,
receipts, commands, or logs.

## Preflight

Preflight MUST fail before mutation when it cannot prove a safe target.

### Host and profile resolution

- Detect supported host installations without launching or controlling their
  UI.
- `--dcc-path` overrides discovery and MUST select the matching host version
  and versioned profile/project layout.
- Record host path, host version, profile/project path, and selection source in
  the plan.
- Reject unsupported host versions against the adapter compatibility matrix.
- Never choose an unrelated newest profile when `--dcc-path` identifies a
  different installed version.

### Interpreter resolution

Resolve the target interpreter in this order:

1. `--python`;
2. `DCC_MCP_INSTALL_PYTHON`;
3. a host-specific embedded or sidecar interpreter discovered from the selected
   DCC;
4. an explicit preflight failure with executable remediation.

Hosted adapters MUST NOT silently fall back to an arbitrary `python` on
`PATH`. Record the executable, version, and resolution source. Verify that the
target interpreter can import the exact adapter version and a Core version that
satisfies `min_core_version` before claiming success.

### Filesystem and current state

- Check parent-directory writability and any project-specific policy boundary.
- Inspect the target, registration files, host enablement, and prior receipt as
  one installation state.
- Detect `partial` state precisely; do not overwrite unknown or unreceipted
  user-owned files.
- Call `inspect_install_root` before removal or replacement of native payloads.
- Report exact locked paths and deferred work when a loaded artifact prevents
  a safe transaction.

## Host enablement

Installation finishes the host-side job. Copying a wheel or plugin directory
alone is not success. The transaction MUST perform and receipt the exact
enablement required by the host, for example a Maya module, Blender add-on
enablement, Houdini package JSON, UXP registration, Godot plugin enablement,
Nuke startup hook, or persistent `KATANA_RESOURCES` entry.

A GUI-only step is allowed only when the host API physically requires user
interaction. Return one structured `next_steps[]` entry with the exact host,
menu/control path, action, and reason. Do not use generic UI automation or
claim completion before that step is verified.

## Transaction and receipt contract

Every mutating install or upgrade MUST be transactional:

1. Create a unique staging directory on the destination filesystem.
2. Acquire pinned artifacts and verify integrity before touching the current
   install.
3. Build the complete extension, registration, interpreter binding, and host
   enablement in staging where the host permits it.
4. Inspect locks and stop only adapter-owned sidecars through explicit safe
   lifecycle contracts. Never terminate the user's DCC process.
5. Move the previous adapter-owned state to a transaction backup.
6. Commit the staged state and receipt atomically where the platform permits.
7. On any failure, remove the new state and restore every previous component,
   including the prior receipt.
8. Remove backups only after commit and required artifact checks succeed.

`safe_remove_tree` and `safe_replace_tree` provide structured lock handling,
but an adapter MUST NOT assume a helper name implies rollback. A delete-then-copy
implementation does not satisfy this transaction by itself. Use it only where
the destination is absent or wrap it in a staged backup/restore transaction.
Never half-delete an existing installation.

Each successful install writes a versioned receipt containing at least:

- schema and adapter version;
- DCC type/version and selected host/profile paths;
- target interpreter path/version and Core version;
- owned files plus content digests;
- registration and host configuration touched;
- server/sidecar binding information needed by verification; and
- transaction time and prior-version/upgrade provenance.

The result always returns `receipt_path`. `uninstall`, `upgrade`, repair, and
rollback consume the receipt rather than rediscovering ownership by filename.
Re-running the same desired version converges without duplicating hooks or
configuration.

Uninstall MUST remove only receipted adapter-owned state and be idempotent when
that state is already absent. It MUST preserve user files and refuse ambiguous
unreceipted deletion. Upgrade MUST restore the prior working version when the
new transaction fails; rollback to "nothing installed" is non-conforming.

## Verify to usable

`install --execute` or adapter `install --yes` automatically chains the same
public verification used by the `verify` verb. Verification runs in this order:

1. receipt and expected-path consistency;
2. owned artifact existence and digest;
3. installed package/version metadata in the target interpreter;
4. adapter import in that exact interpreter;
5. host enablement/configuration and captured bootstrap-error state;
6. when the host can run, bounded readiness through
   `wait_for_sidecar_ready(..., probe_tool=<dcc>_diagnostics__ping)` or an
   equivalently typed, read-only adapter probe.

The `verify` object MUST always report:

- `directly_usable`: true only when every required stage succeeds;
- `failure_stage`: the first failed stage, otherwise null; and
- `failure_reason`: a specific safe diagnostic, otherwise null.

Transport registration, a copied plugin, a running process, or a healthy
gateway alone is not direct usability. If a real host is unavailable in CI,
contract tests MUST exercise the executable boundary and the release notes
MUST retain the live-host gap.

## Bootstrap diagnostics

Every startup hook MUST make early failures observable before the MCP service
exists. Python hosts SHOULD wrap their complete import/start block with
`capture_bootstrap_errors`. Non-Python hosts MUST provide an equivalent bounded,
structured, persistent record containing timestamp, startup stage, error class,
message, and adapter/Core version evidence.

The original host error remains visible. Capture failures are also surfaced;
they MUST NOT swallow or replace the original exception. Successful startup
clears or supersedes stale bootstrap errors so `verify` does not report an old
failure as current. Do not automatically close host dialogs, restart the host,
or retry startup without an explicit operator action.

## Documentation and catalog contract

Every adapter repository MUST have a root `install.md` based on the template
below. It owns host-specific requirements, compatibility, exact commands,
platform differences, and troubleshooting. The README links to it rather than
duplicating the runbook.

The adapter catalog `instructions_url` MUST use the immutable repository path
shape:

```text
https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-<dcc>/main/install.md
```

Do not point the catalog at a weaker README or a mutable media/document host.

## CI acceptance

At minimum, every adapter CI runs:

- schema validation of plan/status/verify/install/uninstall/upgrade JSON;
- plan and `--dry-run` with an assertion that no target or receipt changed;
- synthetic-profile install and receipt creation;
- a second identical install proving convergence;
- injected commit/receipt failure proving the prior state is restored;
- stale interpreter/server/profile diagnostics with stable exit codes;
- receipt-driven uninstall followed by an idempotent second uninstall;
- build/wheel inspection proving host bootstrap and install metadata ship;
- the repository's minimum-Python compatibility gate; and
- a real-host smoke where the runner can legally provide the DCC.

The smoke should cover plan -> execute -> verify -> status -> uninstall. CI
MUST NOT fake readiness or `requires_restart` to turn a missing host green.

## Reusable `install.md` template

Copy [templates/adapter-install.md](templates/adapter-install.md) to the adapter
repository root as `install.md`. Replace every placeholder token and
remove branches that do not apply. Keep all eight required sections even when a
section only states that no extra step is required.

The template is a public runbook, not an implementation substitute. Commands,
paths, probe tools, supported versions, and troubleshooting evidence must match
the adapter's executable tests.

## Deferred Core slices

Issue #2252 tracks later, separately reviewable work for embedded-interpreter
detection in the Rust CLI, `min_core_version` enforcement, JSON execution
reports, idempotent executor rollback, lock-aware removal integration,
first-class Core verify/uninstall, and catalog migration. Adapter conformance
work can use this schema and documentation foundation without waiting for those
executor changes.
