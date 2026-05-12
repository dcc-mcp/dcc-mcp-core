# Verified Regression Suite (VRS)

JSONL traces plus a zero-dependency HTTP replayer to pin **live** gateway / DCC behaviour that unit tests miss.

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

## Traces in this repo

| File | Needs live Maya? | Purpose |
|------|------------------|---------|
| `traces/gateway-smoke.jsonl` | No | `GET /v1/healthz` + `POST /v1/search` against any running gateway. |
| `traces/maya-215-execute-python-regression.jsonl` | Yes | After a harmless `execute_python`, runs a `cmds` call that raises `TypeError`, then requires a **second** harmless `execute_python` to succeed (guards maya#215 / #199 class regressions). |

## CI policy (recommended)

- **Always** run `gateway-smoke` against a test gateway in CI when one exists.
- Run `maya-215-execute-python-regression` only on nightly / studio runners with a live Maya + gateway; export `VRS_GATEWAY_URL` and invoke `just vrs-replay` (or call `scripts/vrs_replay.py` directly).

## Related issues

- maya [#215](https://github.com/loonghao/dcc-mcp-maya/issues/215) — execute_python crash regression
- maya [#199](https://github.com/loonghao/dcc-mcp-maya/issues/199) — prior crash report
