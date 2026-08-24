# Feedback API

Agent feedback and rationale utilities for DCC-MCP servers (issues #433, #434).

Three complementary features: **Rationale capture** — agents include `_meta.dcc.rationale` in `tools/call` requests to explain why they are invoking a tool. **Gateway feedback** — `POST /v1/feedback` and `dcc-mcp-cli feedback` remain available with zero live DCC instances. **Compatibility forwarder** — `dcc_feedback__report` remains available on live adapters and forwards through the same gateway contract.

**Exported symbols:** `FINDING_V1_SCHEMA_VERSION`, `FindingRuntimeContext`, `FindingValidationError`, `build_finding_v1`, `finding_fingerprint`, `finding_v1_json_schema`, `clear_feedback`, `extract_rationale`, `get_feedback_entries`, `make_rationale_meta`, `register_feedback_tool`

## Finding v1 contract

The canonical machine contract is packaged at
`dcc_mcp_core/schemas/feedback-finding-v1.schema.json`. Rust users can read the
same bytes through `dcc_mcp_models::FINDING_V1_JSON_SCHEMA` and use
`FindingV1`; Python users can import `dcc_mcp_core.schemas.finding` and call
`finding_v1_json_schema()` and `build_finding_v1(...)`.

A complete finding has runtime-owned `dcc_type`, adapter/core/host versions and
OS, plus agent-authored `phase`, `severity`, `intent`, `observed`, `expected`,
one `repro.argv` or `repro.steps` list, and a subject identified by `tool_slug`
or `evidence.error_kind`. `phase` is `install`, `startup`, `dispatch`, `skill`,
or `other`; `severity` is `blocker`, `degraded`, `workaround_found`, or
`suggestion`.

The handler derives `fingerprint` as SHA-256 over the normalized owning
repository, phase, tool slug or error kind, and host major version. It attaches
`redaction_status.mode="needs-review"`; this is intentionally not a claim that
the finding is safe to publish. Reproduction lists are limited to 64 items,
text fields to 4,096 characters, identifiers to 256 characters, and serialized
evidence to 32 KiB. Unknown or ambiguous fields fail closed.

## Offline issue routing

Resolve a validated Finding v1 file to its owning public issue tracker without
starting or contacting a gateway:

```bash
dcc-mcp-cli feedback route finding.json --json
# Use a reviewed catalog instead of the bundled public catalog:
dcc-mcp-cli feedback route finding.json --catalog catalog.yml --json
```

The command is read-only: it returns `repo`, `issues_url`, and a stable
`rationale`, but never creates an issue. `install`, `startup`, and `dispatch`
findings route by exact adapter package name in the catalog. Gateway, CLI, and
protocol `evidence.error_kind` namespaces route to `dcc-mcp-core` before that
phase fallback. `other` findings fail closed unless they have one of those
shared error kinds.

Skill findings do not inherit the adapter repository. They require bounded
routing evidence copied from the owning Skill's
`metadata.dcc-mcp.links.repo` and `metadata.dcc-mcp.links.issues` fields:

```json
{
  "evidence": {
    "error_kind": "skill_contract_violation",
    "routing": {
      "source": "skill_metadata",
      "skill_name": "godot-export",
      "repo": "https://github.com/dcc-mcp/dcc-mcp-godot",
      "issues_url": "https://github.com/dcc-mcp/dcc-mcp-godot/issues"
    }
  }
}
```

Missing, duplicate, non-canonical, or conflicting ownership data is an error;
the CLI never guesses another repository. The Finding remains
`redaction_status.mode="needs-review"`, so routing is not publication approval.

## Public-safe feedback bundles

After reviewing a Finding and setting `redaction_status.mode="public-safe"`
with every exclusion flag true, assemble its bounded diagnostic evidence:

```bash
dcc-mcp-cli feedback bundle finding.json --json
# Override discovery when the PID or log root is not in the finding:
dcc-mcp-cli feedback bundle finding.json --dcc-pid 4321 --log-dir /safe/log/root --json
```

`feedback bundle` is read-only and does not auto-start a Gateway. It combines
the reviewed Finding, a redacted `doctor` snapshot, a version matrix, the
stable public-safe issue report when `evidence.request_id` exists, and the
tail of the exact `dcc-mcp-<dcc>.<pid>.host-errors.log` regular file. Host-error
input is capped at 256 KiB and 50 records by default (`--host-error-lines`,
maximum 200); raw messages, tracebacks, metadata, paths, tokens, and the DCC
PID are excluded from output. The PID is resolved from `--dcc-pid` or
`evidence.dcc_pid`; the log root is resolved from `--log-dir`,
`DCC_MCP_LOG_DIR`, or the platform log directory.

The result uses `dcc-mcp.feedback-bundle.v1`. Each component reports
`included`, `not_applicable`, or `unavailable`, so missing evidence is never
silently treated as complete. Current builds mark the install execution report
unavailable until the validated install-report contract is present, therefore
`complete` remains false. There is no raw bundle mode; inspect raw issue-report
exports and host logs locally instead of attaching them automatically.

## Authorized, deduplicated issue filing

Plan an issue operation from a reviewed public-safe Finding without starting a
Gateway:

```bash
dcc-mcp-cli feedback file finding.json --json
# After reviewing the returned next_step and obtaining user authorization:
dcc-mcp-cli feedback file finding.json --existing 42 --yes --json
dcc-mcp-cli feedback file finding.json --create --yes --json
```

The first command is read-only. It routes the Finding, verifies that its
fingerprint belongs to the routed repository, and searches open issues through
`gh`: first by the fingerprint digest, then by bounded title keywords when no
exact match exists. Full fingerprint matching happens locally against returned
titles and bodies. One exact match recommends a comment; zero candidates
recommend creation. Keyword-only, multiple, or truncated candidates require
review and are never selected automatically.

Writing requires both `--yes` and exactly one decision (`--existing <number>`
or `--create`). The CLI repeats exact-fingerprint search immediately before the
write. A new or conflicting exact match, a closed selected issue, invalid
tracker data, missing GitHub authentication, or any search failure stops the
operation. Issue bodies contain only the reviewed Finding v1 projection;
request, job, instance, raw evidence, and extra fields are excluded. Bodies are
passed to `gh` through stdin rather than command-line arguments. This command
does not yet group multiple findings or apply the organization issue form and
labels.

## register_feedback_tool

```python
register_feedback_tool(
    server,
    *,
    dcc_name="dcc",
    gateway_endpoint=None,
    gateway_host=None,
    gateway_port=None,
    instance_id_provider=None,
    finding_context_provider=None,
) -> None
```

Register the `dcc_feedback__report` MCP tool on `server`. Call **before** `server.start()`.

The preferred input is the agent-authored subset of Finding v1 described above.
The original `tool_name`, `intent`, `blocker`, `severity` (`"blocked"` |
`"workaround_found"` | `"suggestion"`), optional `attempt`, and optional
`request_id` / `job_id` form remains accepted and is normalized to Finding v1.
The shared Core handler auto-fills runtime identity and the current
`instance_id`, posts to gateway `/v1/feedback`, validates exact `X-Request-ID`,
schema-version, and fingerprint response correlation, and returns the receipt.
It fails closed when identity is missing or mismatched, the gateway is disabled
or unavailable, validation is rejected, or the receipt is stale/malformed.
There is no instance-local success fallback.

## Agent failure-reporting workflow

Shell-capable agents use the published `dcc-mcp` Skill and existing CLI
surfaces:

```bash
dcc-mcp-cli doctor
dcc-mcp-cli stats --range 24h --status failure --session-id <session-id>
dcc-mcp-cli feedback \
  --tool-name tool_that_failed \
  --intent "goal" \
  --attempt "sanitized attempt" \
  --blocker "observed failure" \
  --severity blocked \
  --dcc-type <dcc> \
  --instance-id <live-or-dead-instance-id> \
  --request-id <request-id> \
  --job-id <job-id>
```

The gateway records a bounded `feedback_reported` entry in
`resources://gateway/events` and returns its `feedback_id`; it does not require
a live DCC and does not create an external issue. Use sanitized values and never
include credentials, reusable tokens, or raw sensitive payloads. For a
gateway-routed failure, preserve the CLI-returned
`request_id` and retrieve public-safe
`/v1/debug/issue-reports/<request_id>`. Review `?mode=raw` locally and never
upload it automatically. Route Skill defects to the owning Skill, adapter or
host-runtime defects to the adapter repository, and shared CLI/gateway/protocol
defects to `dcc-mcp-core`; create an external issue only with user authorization.

Adapters also mirror accepted reports to bounded, rotated JSONL files below the
shared registry directory. Query those durable records through the gateway:

```bash
dcc-mcp-cli feedback list --range 7d --dcc maya --severity blocked --json
dcc-mcp-cli feedback export --range all --dcc maya --json
dcc-mcp-cli feedback route finding.json --json
dcc-mcp-cli feedback bundle reviewed-finding.json --json
dcc-mcp-cli feedback file reviewed-finding.json --json
```

Both commands call `GET /admin/api/feedback`. `list` defaults to 100 rows and
`export` to the endpoint maximum of 1,000. The response is newest first,
deduplicated by feedback id, and reports `skipped_invalid`, `deduplicated`, and
`files_scanned` counters. Malformed or oversized individual records never enter
the result; directory/file I/O errors or exceeded scan bounds fail explicitly
instead of returning a silently incomplete export.

The compatibility `dcc_feedback__report` entry point disappears with its live
adapter, but while live it is only a thin forwarder to the gateway authority.
Prefer the gateway CLI/REST path for crash-class feedback and reference the dead
instance plus its last request/job ids directly.

## extract_rationale

```python
extract_rationale(params: dict | str) -> str | None
```

Extract `_meta.dcc.rationale` from a `tools/call` params dict.

```python
params = {
    "name": "create_sphere",
    "arguments": {"radius": 1.0},
    "_meta": {"dcc": {"rationale": "User wants a reference sphere."}},
}
rationale = extract_rationale(params)  # "User wants a reference sphere."
```

## make_rationale_meta

```python
make_rationale_meta(rationale: str) -> dict
```

Build the `_meta` fragment for a `tools/call` request with a rationale. Returns `{"_meta": {"dcc": {"rationale": "..."}}}`.

## get_feedback_entries

```python
get_feedback_entries(*, tool_name=None, severity=None, limit=50) -> list[dict]
```

Return recent feedback entries, newest first. Gateway-accepted adapter entries
have `id`, `timestamp`, and the complete Finding v1 fields. The legacy local
compatibility handler may still expose the earlier `tool_name` / `blocker`
shape.

## clear_feedback

```python
clear_feedback() -> int
```

Clear all in-memory feedback entries. Returns the count cleared.
