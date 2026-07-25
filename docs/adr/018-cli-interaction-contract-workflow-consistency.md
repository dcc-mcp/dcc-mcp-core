# ADR-018: CLI Interaction Contract & Workflow Consistency

## Status

Proposed

## Context

Automation agents, CI pipelines, and human operators all invoke `dcc-mcp-cli` —
but today each consumer parses output differently because the CLI lacks a
normative output contract. Agents scrape human-readable tables, CI matches
against unstructured JSON, and error handling depends on whether the writer
remembered to exit non-zero. This makes the CLI fragile as a contract surface
between the platform and its consumers.

PIP-2880 (Unity CLI research) confirmed that every mature CLI in the agent-tool
space converges on the same four primitives: structured output channels,
stdout/stderr separation, semantic exit codes, and a machine-readable error
envelope. This ADR codifies those primitives for `dcc-mcp-cli` so that every
command — existing and future — obeys the same contract.

## Decision summary

Introduce a mandatory CLI interaction contract with six normative layers.
Every `dcc-mcp-cli` subcommand MUST comply. The contract is enforced at the
presentation layer (`cli.rs`), not per-command, so new commands inherit it
automatically.

### 1. Three-channel output (`--output`)

| Value    | Target     | Format                           |
| -------- | ---------- | -------------------------------- |
| `human`  | stdout     | Plain text / tables (TTY-safe)   |
| `json`   | stdout     | Single JSON object or array      |
| `ndjson` | stdout     | Newline-delimited JSON stream    |

- **TTY autodetection**: when `--output` is omitted, a TTY-attached stdout
  defaults to `human`; a pipe/redirect defaults to `json`.
- Explicit `--output` always wins over autodetection.
- `ndjson` is for streaming/long-running commands (e.g. `wait-ready --watch`).

### 2. stdout/stderr separation

- **stdout**: machine-consumable data only (JSON, NDJSON, or the human table).
  Guaranteed parseable when `--output json` or `--output ndjson` is active.
- **stderr**: human-facing diagnostics — progress messages, warnings, connection
  retry notices, deprecation hints, gateway auto-start info. Never structural.
- Rationale: `dcc-mcp-cli call --output json 2>/dev/null | jq .result` must
  always work.

### 3. `--non-interactive` mode

A global flag (`--non-interactive` / `-n`) that guarantees zero interactive
prompts. In this mode:

- Missing required input → exit code 2 (`EXIT_INVALID_INPUT`) immediately,
  with a stderr message naming the missing field.
- Confirmation prompts (install, uninstall, destructive operations) →
  treated as declined unless `--force` / `--yes` is also passed.
- stdin is never read for interactive input.
- TTY detection for output format is unaffected (use `--output` explicitly
  when you also need a specific format).

### 4. Semantic exit codes (0–7)

| Code | Name                   | Meaning                                        |
| ---- | ---------------------- | ---------------------------------------------- |
| 0    | `EXIT_SUCCESS`         | Command completed successfully                 |
| 1    | `EXIT_GENERAL_ERROR`   | Unclassified runtime failure                   |
| 2    | `EXIT_INVALID_INPUT`   | Missing/bad arguments, schema validation error |
| 3    | `EXIT_UNAVAILABLE`     | Gateway/DCC instance unreachable               |
| 4    | `EXIT_TIMEOUT`         | Operation timed out                            |
| 5    | `EXIT_CANCELLED`       | SIGINT / user cancellation                     |
| 6    | `EXIT_PERMISSION_DENIED`| Operation blocked by policy or missing grant  |
| 7    | `EXIT_CONFLICT`        | Resource conflict (PID lock, port, instance)   |

- A process that receives SIGINT exits with code 5, not 130. The CLI installs a
  signal handler that translates the signal.
- Exit codes are the outer contract: even if JSON error envelope is also
  printed to stdout (for commands that partially succeeded), the process exit
  code is authoritative for scripting.

### 5. Timeout and cancellation semantics

- **`--timeout-secs <N>`** is a global flag (also settable via
  `DCC_MCP_CLI_TIMEOUT_SECS`). It applies to every network call within a
  command. Per-subcommand timeout flags are deprecated and mapped to the
  global one with a warning.
- A command that hits the timeout exits with code 4 (`EXIT_TIMEOUT`) and
  prints a JSON error envelope to stdout (if `--output json`) plus a
  diagnostic line to stderr.
- **SIGINT (Ctrl+C)**: the CLI installs a `tokio` signal handler. First
  SIGINT → attempt graceful cancellation of in-flight requests; second
  SIGINT → hard exit 5.

### 6. Unified error envelope

Every error that reaches the presentation layer is serialized as:

```json
{
  "error": {
    "code": "GATEWAY_UNREACHABLE",
    "message": "gateway at http://127.0.0.1:9765 did not respond within 5s",
    "exit_code": 3,
    "retryable": true,
    "details": {
      "host": "127.0.0.1",
      "port": 9765,
      "timeout_secs": 5
    }
  }
}
```

| Field        | Type             | Description                                          |
| ------------ | ---------------- | ---------------------------------------------------- |
| `code`       | `string`         | Stable machine-readable error identifier (UPPER_SNAKE) |
| `message`    | `string`         | Human-readable one-line description                   |
| `exit_code`  | `integer`        | The semantic exit code this error maps to             |
| `retryable`  | `boolean`        | Whether the same request may succeed if retried       |
| `details`    | `object` \| null | Optional structured context (field names, values)     |

- `code` values are drawn from a closed enum defined in the CLI crate.
  Introducing a new code is a minor-version event.
- `details` is always an object or null — never a string, never an array.
- When `--output human`, the error is printed as a single stderr line:
  `error: [GATEWAY_UNREACHABLE] gateway at 127.0.0.1:9765 did not respond within 5s`
- When `--output json`, stdout receives the envelope above and stderr is empty.
  This allows `jq '.error.exit_code'` to work reliably.

## Requirements

### Functional

1. `--output` flag MUST accept `human`, `json`, `ndjson` (case-insensitive).
2. TTY autodetection MUST select `human` for TTY stdout, `json` otherwise,
   when `--output` is omitted.
3. `--non-interactive` MUST prevent all stdin reads for prompts and MUST exit
   code 2 when required input is missing.
4. Every error MUST carry a stable `code`, human `message`, `exit_code`,
   `retryable` flag, and optional `details` object.
5. SIGINT MUST produce exit code 5 after the signal handler translates it.
6. `--timeout-secs` MUST be accepted as a global flag; per-command timeout
   flags MUST emit a stderr deprecation warning when combined with the global
   flag.

### Migration

1. Existing `OutputFormat::Json` and `OutputFormat::Pretty` MUST map to
   `json` and `human` respectively, with `Pretty` retained as a hidden alias
   for one release cycle.
2. Existing `process::exit(1)` calls MUST route through the error envelope
   path instead — never call `std::process::exit` directly from command
   handlers.
3. Existing per-command timeout flags (`--timeout-secs` on `call`, `smoke`,
   `wait-ready`, `ui-control`) MUST continue to work but emit a deprecation
   warning on stderr when the global `--timeout-secs` is also present.

### Non-requirements

- This ADR does NOT prescribe the internal error type hierarchy — only the
  presentation-layer envelope shape and the exit code mapping.
- It does NOT require replacing `anyhow` — the error envelope is a
  presentation concern, not a domain-layer constraint.
- It does NOT mandate NDJSON for any specific command today; it only requires
  the `--output ndjson` flag to be accepted and the plumbing to exist.

## Implementation plan

### Phase 1: foundation (this ADR → PIP-2894)

1. Define the `OutputFormat` enum extension and TTY detection in `cli.rs`.
2. Introduce the `ErrorEnvelope` struct and `ExitCode` enum in a new
   `presentation::output` module.
3. Introduce the `--non-interactive` global flag and wire it into the `Args`
   struct.
4. Introduce the `--timeout-secs` global flag.
5. Replace `print_value` with a contract-aware `OutputWriter` that enforces
   stdout/stderr separation.
6. Install the SIGINT handler and translate to exit code 5.
7. Map all existing error paths through the envelope.

### Phase 2: per-command adoption (future)

1. Graduate per-command timeout flags to emit deprecation warnings.
2. Add NDJSON streaming to `wait-ready --watch` and `list --watch`.
3. Add `--force` / `--yes` flags to destructive commands for non-interactive
   compatibility.

## Consequences

### Positive

- Agents and CI systems get a stable, parseable contract independent of the
  command being invoked.
- New subcommands inherit the contract automatically — the `OutputWriter`
  and error envelope are presentation-layer concerns.
- Stderr diagnostics no longer corrupt JSON output piped to `jq` or an agent.

### Negative

- One-time churn: every existing `eprintln!` call must be audited to ensure
  it isn't accidentally printing data, and every `process::exit(1)` must be
  rerouted.
- The `ndjson` channel adds complexity to the output writer (line-delimited
  framing must be correct), though it is deferred to phase 2 for the first
  streaming command.

### Neutral

- `anyhow` remains the internal error propagation mechanism. The error
  envelope is a presentation concern that wraps `anyhow::Error` at the top
  level. Domain code does not need to know about exit codes.

## Alternatives considered

### 1. Use exit codes 64–78 (sysexits.h)

Rejected. sysexits conventions (`EX_USAGE=64`, `EX_TEMPFAIL=75`, etc.) are
not widely recognized outside BSD land, and codes 0–7 are simpler for
agent-automation consumers to branch on.

### 2. Error envelope as HTTP-style problem+json (RFC 9457)

Rejected. RFC 9457's `type`/`title`/`status`/`detail` vocabulary is designed
for HTTP APIs; our CLI consumers need `exit_code` and `retryable` more than
a `type` URI. We add a `code` field for machine matching, which RFC 9457
lacks.

### 3. Print JSON errors to stderr

Rejected. In pipeline mode (`--output json | jq`), consumers expect stdout
to be parseable JSON. Errors on stderr are invisible to `jq` filters that
only consume stdin. The contract is: stdout = structured data, stderr =
human diagnostics. An error IS structured data, so it belongs on stdout.

## References

- PIP-2880: Unity CLI research report (Steve Jobs, 2026-07-24)
- PIP-2881: RFC for CLI interaction contract (parent of this ADR's
  implementation issues)
- PIP-2894: P0 implementation task for this ADR
- [Command Line Interface Guidelines](https://clig.dev/) — stdout/stderr
  separation and exit code conventions
- [12 Factor CLI Apps](https://medium.com/@jdxcode/12-factor-cli-apps-dd3c227a0e46) —
  JSON output and TTY detection patterns
