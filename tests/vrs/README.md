# Verified Regression Suite (VRS)

JSONL traces plus a zero-dependency HTTP replayer to pin **live** gateway / DCC behaviour that unit tests miss.

**Process for contributors** (naming, when to skip, PR checklist) → root [`AGENTS.md`](../../AGENTS.md) section *Verified Regression Suite (VRS)*.

## Trace format

Each trace is a UTF-8 `.jsonl` file:

1. **Optional header line** — object with `"_vrs": {"version": 1, ...}` and metadata:
   - `trace_id` — short stable id (matches GitHub issue when applicable).
   - `skip_preflight` — optional object. If present, the replayer runs one HTTP step first; when `skip_when` matches, the whole trace exits **0** (skipped, not failed). Use this when no live Maya is registered.

2. **Step lines** — each is a JSON object:
   - `id` — optional label for logs.
   - `http` — `{ "method": "GET"|"POST", "path": "/v1/...", "json": { ... } }` (`json` omitted for GET).
   - `expect` — assertions (all must pass), **or** `expect_any` — list of `expect` objects (at least one must pass).
   - `capture` — optional `{ "json_pointer": "/hits/0/tool_slug", "as": "slug" }` after a successful step; substitutes `{{capture:slug}}` in later bodies.

### Expect fields

| Field | Meaning |
|-------|---------|
| `status` | HTTP status int, or list of allowed ints |
| `body_contains` | Substring that must appear in the raw body |
| `json_subset` | Recursive partial match on parsed JSON (dict leaves must match) |

### Substitution

Any string in `http.json` may contain `{{capture:NAME}}` filled from prior `capture.as` values.

## Running the replayer

```bash
python scripts/vrs_replay.py --base-url http://127.0.0.1:9765 \
  --trace tests/vrs/traces/gateway-smoke.jsonl
```

Or via Just:

```bash
just vrs-replay TRACE=tests/vrs/traces/gateway-smoke.jsonl BASE=http://127.0.0.1:9765
```

Environment:

| Variable | Default | Purpose |
|----------|---------|---------|
| `VRS_HTTP_TIMEOUT_SECS` | `120` | Per-request timeout |

Exit codes: **0** success or skip; **1** failure.

Dry-run (parse + print steps; no HTTP):

```bash
python scripts/vrs_replay.py --base-url http://127.0.0.1:1 --dry-run --trace tests/vrs/traces/<file>.jsonl
```

## Traces in this repo

| File | Needs live DCC? | Purpose |
|------|-----------------|--------|
| `traces/gateway-smoke.jsonl` | No | `GET /v1/healthz` + browse `POST /v1/search`. |
| `traces/gateway-search-no-matches.jsonl` | No | Search with improbable token → `total: 0`, empty `hits`. |
| `traces/gateway-rest-describe-bad-request-missing-tool-slug.jsonl` | No | `POST /v1/describe` with `{}` → `400`, `error.kind` = `bad-request`. |
| `traces/gateway-rest-describe-unknown-slug.jsonl` | No | Well-formed unknown slug → `404`, `error.kind` = `unknown-slug`. |
| `traces/gateway-rest-call-missing-tool-slug.jsonl` | No | `POST /v1/call` without `tool_slug` → `400`, `bad-request`. |
| `traces/gateway-rest-call-unknown-slug.jsonl` | No | Unknown slug on call path → `404`, `unknown-slug` (matches refresh-retry semantics). |
| `traces/maya-215-execute-python-regression.jsonl` | Yes (Maya) | After harmless `execute_python`, triggers `TypeError` via bad `polySphere` arg; follow-up call must still succeed (maya#215 / #199 class). |

## CI policy (recommended)

- **PR CI**: dry-run over every `tests/vrs/traces/*.jsonl` (see root `ci.yml` Lint job).
- **Live**: run `gateway-smoke` + REST error traces against any test gateway when available.
- **Maya-specific**: run `maya-215-execute-python-regression` on nightly / studio runners with live Maya + gateway; use `skip_preflight` so absent hosts skip cleanly.

## Related issues

- maya [#215](https://github.com/loonghao/dcc-mcp-maya/issues/215) — execute_python crash regression
- maya [#199](https://github.com/loonghao/dcc-mcp-maya/issues/199) — prior crash report
