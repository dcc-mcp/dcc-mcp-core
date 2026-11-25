# Offline evaluation

The core evaluation runner provides a deterministic, DCC-free contract for
modeling verification. It consumes Python case modules and emits a versioned
JSON report.

## Run the standard core cases

```powershell
$env:PYTHONPATH = "python"
vx python scripts/run_eval.py run `
  --module tests/eval/cases/core_cases.py `
  --output artifacts/core-eval-report.json `
  --set-version eval-set-v1 `
  --generated-at 2026-01-01T00:00:00Z
```

The standard set contains 20 editor-free cases. Fifteen cases exercise passing
verification paths. Five cases are deliberately discriminating and carry named
defect reasons. A failing case must represent a real verification defect, not a
missing fixture or an unavailable host.

## Validate and compare reports

```powershell
$env:PYTHONPATH = "python"
vx python scripts/run_eval.py validate artifacts/core-eval-report.json --kind report
vx python scripts/run_eval.py diff artifacts/previous.json artifacts/core-eval-report.json
```

`diff` compares the original bytes before rendering a unified text diff. This
keeps line-ending changes visible in reproducibility checks.

The report and case schemas are packaged at
`dcc_mcp_core/schemas/eval-report-v1.schema.json` and
`dcc_mcp_core/schemas/eval-set-v1.schema.json`. The public loaders are
`eval_report_json_schema()` and `eval_set_json_schema()`.

## Contribute adapter cases

An adapter repository can provide a module that exposes `CASES` or a `cases()`
function. Each item must be an `EvalCase` with a stable identifier, a useful
title, a pure grader, and optional invocation metadata and tags.

Graders may return a `BehaviorReport`, a mapping with `checks`, or an existing
verification result. Every accepted check must contain a non-empty `name` and a
boolean `passed` field. Grader errors become `inconclusive` runs so the report
cannot silently claim a pass.

Cases must use editor-free fixtures for offline runs. A live-DCC case belongs in
an adapter-specific acceptance lane and should report `inconclusive` when the
required host evidence is unavailable.

Reports are report-only during this phase. They do not grant tool authority or
replace deterministic host validation.
