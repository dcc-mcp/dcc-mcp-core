# Canonical Tool Result Envelope

Status: Accepted

## Context

Python tool handlers exposed three incompatible result shapes. The pure-Python
builder used a string `error` and omitted empty fields, skill helpers used a
string `error` and retained empty fields, while in-process execution returned a
mapping in `error`. The public name `ToolResult` also referred both to the
Rust-backed runtime model and to the unrelated pure-Python wire builder.

Consumers therefore could not safely interpret `error`, and native validation
moved an otherwise valid top-level `_meta` object into `context` while the
source-only Python path preserved it.

## Decision

- `dcc_mcp_core.ToolResult` remains the Rust-backed runtime model.
- The pure-Python wire builder is named `ToolResultEnvelope`. The legacy
  `dcc_mcp_core.result_envelope.ToolResult` import remains as a deprecated,
  behavior-compatible wrapper for one migration window.
- Every domain tool result uses the same fields: `success`, `message`, `error`,
  `prompt`, `context`, optional `postcondition`, and optional `_meta`.
- `postcondition` is a mapping of mutation readback evidence. Its reserved
  `verified` member is a boolean when present. `skill_success(verified=...)`
  writes that marker without placing it in `context`; omitting the argument
  preserves the released fixed-key projection and does not imply failure.
- `error` is either a stable string code or `None`. Structured error details
  use the namespaced `_meta["dcc.error"]` object; raw call diagnostics continue
  to use `_meta["dcc.raw_trace"]`.
- The Rust model preserves `postcondition` and `_meta` during validation and
  JSON/MessagePack serialization without changing the public
  `ActionResultModelData` struct layout. Skill subprocesses use the
  dependency-light normalizer and standard library JSON in every installation
  so native and source-only projections are identical, including tuples and
  integers outside the serde 64-bit range.
- Field presence is a projection concern. The general builder omits empty
  fields by default, while released skill helpers retain their historical
  fixed-key projection for compatibility.
- Exception helpers mirror their released diagnostic context keys for one
  migration window, but `_meta["dcc.error"]` is the canonical structured
  location for new consumers.
- JSON-RPC and host-RPC transport errors remain separate outer envelopes. A
  domain tool result may be nested under a transport `result`, but it must not
  replace the transport's `{result}` / `{error}` contract.

## Consequences

- Consumers may always treat a non-null `error` as a string and inspect `_meta`
  only when structured diagnostics are needed.
- Existing skill scripts that index `error`, `prompt`, or `context` directly
  keep working.
- Consumers can distinguish `postcondition.verified=false` from a legacy or
  unreported verification state where `postcondition` is absent.
- Existing Rust callers may continue constructing `ActionResultModelData` with
  exhaustive struct literals; metadata is held privately by `ActionResultModel`.
  Existing JSON and MessagePack payloads remain compatible.
- New code must use the unambiguous `ToolResultEnvelope` name for wire
  dictionaries.

## Alternatives considered

### Keep structured mappings in `error`

Rejected because it preserves the consumer ambiguity that prompted the
decision and conflicts with the established Rust and skill-helper contracts.

### Normalize only in the pure-Python serializer

Rejected because native validation would still relocate `_meta`, leaving two
wire contracts depending on whether the compiled extension is installed.

### Rename the Rust-backed `ToolResult`

Rejected because it is the established top-level public API and the return type
of runtime factories and deserializers. Renaming the smaller pure-Python surface
has a bounded compatibility path.
