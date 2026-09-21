# RFC 0005 - Modeling Quality Capabilities: Ranking and First Experiment

**Status**: Draft
**Target repo**: `dcc-mcp-core`
**Authors**: dcc-mcp-core contributors
**Date**: 2026-09-21
**Depends on**: no new runtime surface. It consumes
`dcc_mcp_core.verification`, the state-export schemas in
`dcc_mcp_core.verification.schemas`, and the experiment-event model in
[ADR-019](../adr/019-reproducible-agent-experiments.md).

---

## Summary

Five capabilities are on the long-term modeling roadmap: VLM question-based
review, an offline eval set, library learning, API-doc RAG, and a layout
solver. All five claim to improve modeling quality. None of them can currently
be shown to do so, because we have no instrument that would notice if they
did.

This RFC ranks the five and picks **one** first experiment. It does not
propose to build all five, and it does not present them as equally valid
starting points.

**Ranking, best first:**

| Rank | Capability | One-sentence verdict |
| ---- | ---------- | -------------------- |
| 1 | Offline eval set | Build it first: it is the only item whose output makes the other four decidable, and it is pure library work on primitives we already ship. |
| 2 | VLM question-based review | Build it second, and only as an advisor against the eval set: highest raw value of the five, but worthless as a first experiment because there is nothing to measure it against. |
| 3 | API-doc RAG | Build it third on the existing embedder surface: real, frequent payoff (fewer hallucinated host API calls), but it improves authoring convenience, not shipped-asset quality. |
| 4 | Library learning | Defer: it needs a volume of clean, labeled execution history we do not have, and a bad learned library silently degrades the skill catalog in a way that is expensive to unlearn. |
| 5 | Layout solver | Defer last: narrowest applicability, highest build cost under the Python 3.7 red line, and lowest reversibility of the five. |

**Recommendation, stated without hedging: build the offline eval set first.**

## Motivation - why the ranking is the deliverable

The five items are usually presented as a menu. They are not. Four of them
(VLM review, RAG, library learning, layout solver) are *interventions*: each
changes how a model behaves and asserts that the change is an improvement.
The fifth (offline eval) is an *instrument*: it does not change behavior at
all, it makes a claim about behavior testable.

Shipping an intervention before the instrument produces a familiar outcome.
Somebody adds VLM review, a few anecdotes go well, and six weeks later nobody
can say whether it caught a single defect that deterministic checks missed,
because nothing recorded the baseline. The work is then defended by taste,
which is how it gets reverted in a refactor.

The ranking below is therefore weighted for *first experiment*, not for
lifetime value. A high-value, hard-to-reverse capability is a worse first
experiment than a medium-value, trivially reversible one.

## What core already ships

This ranking is not a guess about cost, because most of the deterministic
evidence layer already exists and is shipped in both the native and
`py37-lite` wheels.

| Primitive | Module | What it already gives us | What is missing |
| --------- | ------ | ------------------------ | --------------- |
| Image statistics | `verification.image_stats` (`compute_image_stats`, `classify_image_stats`) | Mean/stddev luma, histogram, uniformity, and bad-frame flags (white playblast, near-black render, missing display transform) | No aggregate scorecard across cases |
| Pixel metrics | `verification.pixel_metrics` (`phash_64`, `dhash_64`, `ahash_64`, `ssim`, `silhouette_iou`, `delta_e_2000`, `edge_density`, `sobel_edges`) | Deterministic reference-vs-render comparison | Same: callable, not assembled |
| Scene-vs-spec | `verification.scene_spec` (`validate_scene_vs_spec`) | Missing-UV, missing-part, unbound-material, non-manifold, Euler checks before export | No fixed case list to run it over |
| Behavior assertions | `verification.assertions` (`BehaviorVerifier`, `BehaviorReport.pass_rate`) | Exact counts, tolerance bands, existence and resolution checks, with a collecting reporter | No persisted run history |
| State-export schemas | `verification.schemas` (`dcc-mcp/anim-curves@1`, `rig-state@1`, `sim-status@1`, `graph-state@1`) | Versioned, dependency-free payload validation | No expectation records keyed to cases |
| Product acceptance | `verification.acceptance` (`evaluate_acceptance`, `build_report`, `dumps_report`) | Fail-closed level chain and a report with injectable `generated_at` | It is a product matrix, not a task-level eval |
| Declaration lint | `verification.lint` (`find_unpaired_write_verbs`, `lint_tool_table`) | Catches write verbs with no paired read-only state export | Not wired into any score |
| Embedding | `vector_embedder` (`HashedEmbedder` zero-dep, `OnnxEmbedder` optional) | A retrieval backend with no required dependency | No host-API corpus |
| Spatial convention | `spatial` (`plan_spatial_conversion`) | Deterministic coordinate conversion for interchange | No solver |
| Experiment events | [ADR-019](../adr/019-reproducible-agent-experiments.md) | `experiment.created`, `experiment.run.<status>`, `experiment.judge.result` with `authority: evidence_only` | No concrete evaluator filled in; ADR-019 explicitly rejected a generic framework "until more than one concrete evaluator needs a shared execution contract" |
| Verification skill | `python/dcc_mcp_core/skills/verification` (`capture_review_views`, `image_stats`, `validate_scene_vs_spec`, `make_comparison_sheet`) | Agent-callable, read-only, DCC-agnostic evidence packaging | Computes no scores, by design |

The gap is narrow and specific: **no fixed list of cases with recorded
expectations, and no single comparable number across them.** That gap is the
offline eval set. Closing it is days of library work over code that is
already written, tested, and Python 3.7 clean.

## How to read the ranking

Four axes, one line each, applied identically to all five:

- **Value** - does it change what ships, either by catching a defect class
  that today reaches the artist, or by shortening an iteration loop?
- **Cost** - calendar time plus new runtime surface: dependencies, model
  access, GPUs, per-host adapter code.
- **Dependency** - what must already be true for the work to be meaningful.
- **Reversibility** - can we delete it later without a data migration, a
  catalog repair, or an adapter release?

Dependency and reversibility are weighted above raw value. Rank 1 is chosen
because it is first, not because it is best.

## The ranking

### 1. Offline eval set - recommended

**Verdict: build first.** It is the only one of the five that costs no new
runtime surface and no external access, and the only one that converts the
other four from opinions into measurable changes.

- **Value.** High, and mostly indirect: it is the instrument. Its direct value
  is regression detection on the modeling path, which today is caught by an
  artist looking at a playblast.
- **Cost.** Low. Pure standard library, no model, no network, no GPU, and it
  is assembled from modules that already exist and are already tested.
- **Dependency.** None. It needs fixtures, and `acceptance_fixtures.py` shows
  the editor-free fixture pattern already works here.
- **Reversibility.** Total. It is additive files plus a test. Deleting it
  breaks nothing that shipped.

### 2. VLM question-based review

**Verdict: build second, as an advisor only - it is the highest-value item in
the set and still the wrong first experiment.**

- **Value.** Highest of the five. It is the only candidate that can address
  the defect class the deterministic layer structurally cannot: "the export
  is valid and the frame is not blank, and it is nevertheless wrong." Every
  other item on this list is about producing or retrieving better inputs.
- **Cost.** Medium-high. Model access and per-call spend; a non-deterministic
  component in a pipeline whose entire value proposition is determinism;
  scene content leaving the machine, which forces a redaction story; and it
  cannot run inside a Python 3.7 DCC host, so it needs an out-of-process
  path.
- **Dependency.** Hard dependency on rank 1. Without a deterministic baseline
  per case, a VLM verdict is unfalsifiable: agreement with the checks is
  redundant, and disagreement is uninterpretable without a ground truth.
- **Reversibility.** Easy while advisory. It becomes irreversible the moment
  its verdict is allowed to gate a run, which [ADR-019](../adr/019-reproducible-agent-experiments.md)
  already forbids (`authority: evidence_only`).
- **Reconsider when.** Any of: the eval set shows a defect class that
  deterministic graders systematically miss and a human can name from the
  artifacts; a studio supplies labeled captures where the artist verdict and
  the deterministic verdict disagree; or VLM review becomes cheap and local
  enough to run per commit.

### 3. API-doc RAG

**Verdict: build third, on the embedder surface we already ship.**

- **Value.** Medium and real, but aimed at a different failure: hallucinated
  or misspelled host API calls. That failure is frequent and cheap to detect,
  which is why it is worth doing - but it makes the author faster, not the
  asset better.
- **Cost.** Medium-low on the retrieval side, medium on the corpus side. The
  retrieval path exists (`HashedEmbedder` needs no dependency at all);
  curating and versioning per-host API corpora is the actual work, and it
  recurs forever because host docs move.
- **Dependency.** Low. It needs the eval set only to prove it helped, and
  retrieval hit-rate can become one eval dimension.
- **Reversibility.** High. An additive index that can be dropped.
- **Reconsider when.** Tool-call error logs show API-name and signature
  mistakes as a top failure mode; or an adapter repo asks for it with a
  corpus we do not have to build ourselves.

### 4. Library learning

**Verdict: defer. The input it needs does not exist yet.**

- **Value.** Medium and long-dated. Compounding returns once it works, which
  is exactly the property that makes it seductive before it is earned.
- **Cost.** High. It needs a large volume of clean, attributed execution
  history, and we are still standing up basic feedback ingest. Garbage in
  here is not a bad result; it is a bad library that looks like a good one.
- **Dependency.** Depends on rank 1 twice: to validate a learned artifact
  against a fixed case list before publication, and to detect the regression
  after publication.
- **Reversibility.** Poor, and this is the deciding axis. Learned libraries
  enter the skill catalog, get selected, get built on, and get pinned. A
  contaminated entry is discovered late and removed with a migration.
- **Reconsider when.** We have enough labeled executions to measure a
  candidate library against before publishing it; and the eval set has been
  stable across a release, so a regression can be attributed to the library
  rather than to case churn.

### 5. Layout solver

**Verdict: defer last. Narrow, expensive, and the least reversible of the
five.**

- **Value.** Narrow. It matters for layout-shaped tasks in hosts that can
  express them, and is irrelevant to the rest of the surface. There is no
  evidence yet that layout is where our output quality is actually lost.
- **Cost.** Highest. A solver means either a new dependency or a large body of
  numerical code, and the Python 3.7 red line (Maya 2022 / Blender 2.83) rules
  out the obvious numerical stack in the `py37-lite` wheel.
  `spatial.plan_spatial_conversion` covers coordinate conversion, not
  constraint solving.
- **Dependency.** Depends on rank 1 to define what "correct layout" means
  measurably, and on adapter-side constraint exports that no adapter ships
  today.
- **Reversibility.** Lowest. Solver output tends to become a scene-authoring
  dependency as soon as artists rely on it.
- **Reconsider when.** A studio names layout as a measured bottleneck with
  artifacts attached; or an adapter ships a constraint export that a solver
  could consume, which is the real prerequisite and is currently absent.

## Recommended first experiment: offline eval set

### Scope, stated as a negative

Not a framework, not a service, not a leaderboard, and not a model in the
loop. It is a fixed list of cases, each with recorded expectations, graded by
code we already have, producing one report per run.

This deliberately follows [ADR-019](../adr/019-reproducible-agent-experiments.md),
which rejected a generic evaluator framework until more than one concrete
evaluator needs a shared execution contract. The eval set is the first
concrete evaluator.

### Inputs

- **Case list.** 20 to 30 cases. One case is: a task identifier, the
  command or skill invocation, the evidence artifacts it should produce
  (capture, state export, or both), and an expectation record.
- **Expectations.** Expressed only in terms the existing primitives can
  grade: schema validity (`assert_state_schema`), structural counts
  (`BehaviorVerifier.exact`), tolerance bands (`within`, `within_band`),
  existence and resolution (`exists`, `resolution`), image statistics
  (`classify_image_stats`), and reference-vs-render metrics (`ssim`,
  `silhouette_iou`, `phash_64` with `hamming_distance`).
- **Fixtures.** Editor-free, following the `acceptance_fixtures.py` pattern
  so the whole set runs on a laptop with no DCC installed.

### Minimal design

```text
EvalCase   { id, title, dcc_type?, invocation, evidence[], expectations[], tags[] }
EvalRun    { case_id, verdict, checks[], duration_ms, artifacts[] }
EvalReport { set_version, generated_at, cases[], pass_rate, failure_histogram }
```

- `verdict` is one of `pass`, `fail`, `inconclusive`. `inconclusive` is not a
  soft pass: a case that cannot be graded is a bug in the case, and it counts
  against the run in the failure histogram.
- The aggregate is `pass_rate` plus a per-check failure histogram. There is
  deliberately **no single weighted score in v1**. A weighted score is a
  number people optimize, and optimizing the metric is the fastest way to
  make an eval set stop measuring anything.
- Reports are JSON, versioned, and byte-stable: the runner accepts an injected
  `generated_at`, the same mechanism `build_report` already uses, so two runs
  on one commit can be diffed.

### Success criteria

The experiment succeeds when all four hold:

1. The set runs headless, with no DCC installed, in under 60 seconds on a
   laptop.
2. It contains at least 20 cases, every one graded deterministically.
3. At least 5 of those cases **fail** on today's code. A set that passes
   cleanly has no discriminating power and has proven nothing. Every failing
   case must carry a one-line **named defect**: what is wrong, and what change
   would fix it. A case that fails for an unrelated reason — a missing
   fixture, a schema version mismatch, an export no adapter implements yet —
   is not a discriminating case; it is fixed or dropped, and it does not count
   toward the 5.
4. Two consecutive runs on the same commit produce byte-identical reports.

Failing cases are consumed, not accumulated. When a named defect is fixed, the
case that exercised it stops discriminating and is replaced by a new case that
still fails, so the number of discriminating cases stays at or above 5. Without
this rule, criterion 3 turns false at the exact moment the set starts doing its
job and the eval set quietly invalidates itself.

### Stop conditions

Stop and re-scope, rather than extending the schedule, if any of these is
true:

- Fewer than 20 cases can be graded without a live DCC in two weeks.
- Fewer than 5 cases discriminate (the set passes cleanly, or fails
  everywhere for one shared reason).
- Non-determinism cannot be removed without pinning a specific DCC version,
  which would make the set a per-host maintenance cost instead of a contract.
- No adapter contributes or adopts a case within one release cycle. A core-only
  eval set measures core, which is not the claim we are trying to test.

### Data and compute

None of the scarce kind. No GPU, no model, no network, no studio data. Laptop
CPU, minutes per run. The real cost is human: roughly one week authoring
cases and three days wiring the runner, and the case-authoring week is the
part that produces the value.

Python 3.7 compatibility is a hard constraint and is achievable by copying
the constraints the sibling verification modules already hold: standard
library only, no third-party imports, no compiled extension.

### Landing: core, not adapter

**Core owns the runner and the schema. Adapters contribute cases.**

The graders are DCC-agnostic by construction - that is already true of
`image_stats`, `pixel_metrics`, `scene_spec`, and the four state-export
schemas. If the runner lands per adapter, we get N incompatible runners, N
incompatible report shapes, and no comparable number, which is precisely the
thing the eval set exists to produce. Adapters stay owners of their own
fixtures and cases, because only they know what a correct export looks like.

New files, all in core:

- `python/dcc_mcp_core/verification/evalset.py` - `EvalCase`, `EvalRun`,
  `EvalReport`, `run_eval_set()`, and the graders-to-checks adapter.
- `python/dcc_mcp_core/schemas/eval-set-v1.schema.json` and
  `python/dcc_mcp_core/schemas/eval-report-v1.schema.json` - versioned case
  and report contracts, packaged like the existing schemas so adapters can
  validate in their own CI.
- `scripts/run_eval.py` - CLI: run a directory of cases, emit a report,
  diff two reports.
- `tests/eval/cases/` - the seed cases.
- `tests/test_evalset.py` - runner and determinism tests.
- `docs/guide/offline-eval.md` - the contract, and how an adapter contributes
  a case.

Edits to existing files: `python/dcc_mcp_core/verification/__init__.py` and
`python/dcc_mcp_core/schemas/__init__.py` re-export the new surface, matching
how the current verification exports are arranged.

### Minimal first step, dispatchable as one issue

One to two days, no CLI, no schema file, no adapter integration:

1. Add `python/dcc_mcp_core/verification/evalset.py` with `EvalCase`,
   `EvalRun`, `EvalReport`, and `run_eval_set()`.
2. Add `tests/test_evalset.py` with 5 seed cases built from editor-free
   fixtures, graded through `BehaviorVerifier`,
   `validate_scene_vs_spec`, and `classify_image_stats`.

**Stop-loss on the first step:** if the 5 seed cases cannot be graded without
a live DCC, stop. That result means the fixtures are the real dependency, and
the next unit of work is fixture extraction, not the runner.

## How the eval set changes the other four

None of the deferred items becomes easier, but all of them become
*judgeable*. That is the entire return on building the instrument first.

| Capability | What the eval set supplies | Gate it must pass |
| ---------- | -------------------------- | ----------------- |
| VLM review | A per-case deterministic baseline and a disagreement ledger | Counted only when it disagrees with the deterministic verdict **and** is right; otherwise it is redundant |
| API-doc RAG | Retrieval hit-rate as one more eval dimension | Pass rate on API-invocation cases does not regress, and hit-rate improves |
| Library learning | A fixed case list to validate a candidate library against before publication | Published only when pass rate does not regress across the set |
| Layout solver | Constraint-satisfaction checks as an eval dimension | Solver output beats the hand-authored baseline on its own dimension |

## Non-goals

- No model in the loop in v1. The deterministic layer runs first and alone;
  that is the point of the ordering.
- No leaderboard, no cross-studio comparison, no public scores.
- No new service, database, or persistence model. Reports are files.
- No change to acceptance authority. Per
  [ADR-019](../adr/019-reproducible-agent-experiments.md), judge output cannot
  approve a run, widen tool authority, or replace deterministic DCC
  validation. The eval set reports; it does not gate, at least for one
  release.
- No changes to existing verification modules. `evalset.py` consumes them.

## Constraints

Same as RFC 0001 through 0003, plus:

1. **Python 3.7 red line** (Maya 2022 / Blender 2.83): standard library only,
   no `requires-python` bump, no CI matrix narrowing, and it must import
   cleanly under the `py37-lite` wheel.
2. **No new required dependency.** Optional extras stay optional; the default
   path stays zero-dep, as `HashedEmbedder` already demonstrates.
3. **Offline by default.** An eval run performs no network access. Any future
   model-backed grader is opt-in and out-of-band.
4. **Additive and opt-in.** No change to existing gateway, adapter, or tool
   behavior.

## Open questions

1. **Case provenance.** Real studio runs carry the defects we care about and
   carry scene content we cannot commit. Synthetic fixtures are safe and may
   be unrepresentative. Suggestion: synthetic in v1, with a documented path
   for studios to contribute redacted cases later.
2. **Where adapter cases live.** Core `tests/eval/cases/` is easy and couples
   adapters to core's release cadence; per-adapter case directories are
   cleaner and need a `--case-dir` flag. Suggestion: adapter repos own their
   cases, core owns only the schema and the flag.
3. **Report persistence.** Files in v1, or straight into ADR-019
   `experiment.judge.result` events? Suggestion: files in v1, event bridge
   once there is a second evaluator.
4. **Case versioning.** A versioned `eval-set-v1` schema, or plain JSON
   validated at load? Suggestion: validate against the schema, so an adapter
   can check its own cases in its own CI without running core.
5. **CI posture.** Hard gate or report-only? Suggestion: report-only for one
   release, then gate on the discriminating subset only - gating on the whole
   set turns a flaky case into a blocked pipeline.

## Phasing

| Phase | Content | Exit |
| ----- | ------- | ---- |
| P0 | `evalset.py` plus 5 seed cases and determinism tests | 5 cases graded with no DCC, two runs byte-identical |
| P1 | `eval-set-v1` / `eval-report-v1` schemas and `scripts/run_eval.py` | An adapter can validate its cases in its own CI |
| P2 | 20 or more cases, at least 5 discriminating, adapter-contributed | Success criteria 1 through 4 all hold |
| P3 | Report-only CI job, then gate on the discriminating subset | One real regression caught, or the set is retired |

## References

- [ADR-019: Reproducible agent experiments](../adr/019-reproducible-agent-experiments.md)
- [ADR-017: Record / replay visual closed loop](../adr/017-codex-record-replay-visual-closed-loop.md)
- [Cross-DCC verification guide](../guide/cross-dcc-verification.md)
- [RFC 0003: Traffic interception and replay](./0003-traffic-interception-and-replay.md)
- [Observability collection overhead baseline](../overhead-baseline.md)
