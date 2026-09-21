# RFC 0004 - Modeling Spec Gate and Acceptance Contract

**Status**: Draft
**Target repo**: `dcc-mcp-core` (consumed by downstream adapters through the flat `dcc-mcp-core` dependency)
**Authors**: dcc-mcp-core contributors
**Date**: 2026-09-21
**Depends on**: None. This RFC touches only the verification schema family and the declaration lint, and adds no runtime dependency on RFC 0001-0003.

---

## Summary

A strict spec gate for modeling output should be built as **typed, reported, and
lint-enforced — not as a runtime block**. Concretely: publish two missing
versioned documents (`dcc-mcp/model-state@1`, `dcc-mcp/model-spec@1`), replace
the gate's boolean verdict with a three-state one, and enforce adoption through
the existing declaration lint. Do **not** interpose a blocking gate in the
dispatch path, do **not** enforce `output_schema` on every tool, and do **not**
build the prompt-to-spec converter in this slice.

The headline finding, stated up front because it changes the shape of the work:
**the spec gate is not missing, it is untyped and unenforced.** The evaluator,
the readback-pairing lint, and input-schema enforcement all already exist. What
is absent is the vocabulary for what the evaluator consumes, the schema for what
it is configured with, and any code path that invokes it. Building a blocking
gate is the expensive part and is also the wrong shape; making the existing one
impossible to fool is cheap and reversible.

## Context

The multi-DCC autonomous modeling evaluation that motivates this roadmap
(#2263) produced 66 defects and one cross-cutting verdict: the typed layer is
neither complete enough to model with (25%-87% of calls fell back to raw
Python) nor trustworthy enough to build on (typed tools returning success while
doing nothing; seven silent-success defects).

Four facts about the current tree determine the design.

**1. The evaluator exists and is never invoked automatically.**
`dcc_mcp_core.verification.scene_spec.validate_scene_vs_spec` compares a
normalised scene export against a written spec across six checks — parts,
hierarchy, materials, uv_coverage, non_manifold, euler — and returns
`{passed, checks, failures}`. It is exposed as the skill tool
`verification__validate_scene_vs_spec`, declared `read_only: true`. Its only
callers are that tool entry point (an agent must choose to call it) and its own
tests. Nothing runs it as a gate.

**2. The scene it consumes has no schema.** The `scene` argument is an ad-hoc
mapping documented in a docstring (`parts`, `hierarchy`, `meshes`, `objects`).
Every adapter invents it; nothing validates it. The versioned state-export
family — `dcc-mcp/anim-curves@1`, `rig-state@1`, `sim-status@1`,
`graph-state@1` — covers animation, rigging, simulation, and node graphs.
**It has no geometry/modeling member**, which is the one domain this gate is
about.

**3. The spec it is configured with has no schema either.** `tools.yaml`
declares the `spec` argument as `{"type": "object"}`. A caller who misspells
`min_uv_coverage`, or nests `required_parts` under the wrong key, receives
`passed: true` with the silent default floor applied. The evaluation recorded
31% of meshes shipping without UVs; the gate configured to catch exactly that
class can be silently disarmed by a typo.

**4. The enforcement machinery already exists, split unevenly across the two
boundaries.** Call arguments are validated against `input_schema` at dispatch:
`select_strategy` returns a `NoOpValidator` when no `ToolMeta` is present, or
when the schema is null, empty, or the default schema **and**
`skip_empty_schema_validation` is set; every other case gets a
`SchemaValidator`. Note that the default schema is folded into the same
"empty" test, so it only reaches `NoOpValidator` when the flag is also set.
`output_schema` is a first-class declaration field on `ToolDeclaration`, spelled
`output_schema` in `tools.yaml` — the camel-case `outputSchema` is only the MCP
wire spelling and is explicitly rejected as a `tools.yaml` key — but **no code
path validates a result against it**; the gateway, gateway-core, and
http-server builders all pass `None`, and the one call site that does pass a
real schema (`script_materialization_tools`) is never validated against it.
Separately, `dcc_mcp_core.verification.lint.find_unpaired_write_verbs` already
implements a mechanical declaration rule that every write verb must ship a
paired read-only state export. That lint is library-only: nothing invokes it in
CI or in the skill linter.

So the input boundary is enforced, the output boundary is declared but dead,
the readback-pairing rule is written but unwired, and the gate itself is typed
on neither side.

## Decisions

### D1 — Publish `dcc-mcp/model-state@1`, the modeling state export — GO

Add the missing member of the existing `dcc-mcp/<domain>-state@1` family.

**Content.** The `schema_name` / `schema_version` identity pair every sibling
carries, then `parts`, `hierarchy`, `meshes` (name, uv_sets, material,
is_non_manifold, vertex_count, bounds), and `objects` (name, transform, and an
optional `euler` triple). Plus two fields the ad-hoc dict cannot express and
the verdict depends on:
`partial` (boolean) and `unavailable` (array of check names the adapter cannot
report).

**Why.** The gate's input is the one document no schema covers today. The
`unavailable` field is the point: without it, an adapter that cannot report UVs
is indistinguishable from an asset that has them, and the gate reports success
either way.

`objects[*].euler` is optional but must be declared, because the Euler check
skips any object that omits it. An adapter that follows a contract naming only
`name` and `transform` would therefore pass the Euler check without a single
value ever being compared. Declaring the member is what makes its absence a
visible gap rather than a silent pass.

**Cost.** One JSON Schema document plus one validator function plus tests,
reusing `_check_schema_identity`, `_require_non_negative_int`, and the
packaging convention in `dcc_mcp_core/schemas/`. No new machinery.

**Reversibility.** Fully additive. Version 1, no consumer yet; removal is a
revert. **Dependency.** None.

### D2 — Publish `dcc-mcp/model-spec@1`, the gate's own input contract — GO

This is the highest-value item in the RFC and the cheapest.

**Content.** `additionalProperties: false`; a required `checks` array naming
which checks the caller *intends* to run, with `minItems: 1`; and the per-check
configuration keys the evaluator reads as top-level members of the spec:
`required_parts`, `required_hierarchy`, `required_materials`,
`min_uv_coverage`, `allow_non_manifold`, and `euler_max_abs_degrees`.

An empty `checks` array is rejected. Under D3 an empty set has no `fail` and no
`unknown` result to roll up, so it would collapse to `pass` — the exact
silent-success class this RFC exists to remove.

The configuration keys must be declared here or `additionalProperties: false`
will reject the spec documents `validate_scene_vs_spec` already accepts today,
which read those values from the top level of `spec` rather than from the
`checks` entries. Their current defaults are the evaluator's own:
`min_uv_coverage` 0.9, `allow_non_manifold` false, `euler_max_abs_degrees`
360.0, and an empty list for each `required_*` key. Step 1 must carry these
defaults forward so publishing the schema does not silently change the
behaviour of an existing gate call.

**Why.** A gate that can be silently misconfigured is worse than no gate. It
does not merely fail to catch a defect — it manufactures a positive verdict,
which is the precise failure class (#2263's silent successes) the gate exists to
remove. `additionalProperties: false` turns a misspelled key from a silent
default into a hard error. The required `checks` array makes "I did not enforce
UV coverage" a declaration rather than an accident.

**Cost.** One schema document plus one validation call. No adapter change.

**Reversibility.** Additive and versioned. **Dependency.** None.

### D3 — Replace the boolean verdict with pass / fail / unknown — GO

`validate_scene_vs_spec` returns `passed: bool`. When `meshes` is absent, the
UV-coverage check returns no failures and the verdict is `passed: true` — an
adapter that exports nothing passes every check.

**Decision.** Return `status` drawn from `pass`, `fail`, `unknown`, computed
per check and rolled up: any `fail` wins, otherwise any `unknown` wins,
otherwise `pass`. Retain `passed` as `status == "pass"` for one migration
window, the same deprecation shape ADR-022 used for the legacy `ToolResult`
import: a behavior-compatible alias, one migration window, and a
`DeprecationWarning` emitted on read so the remaining callers can be counted
instead of guessed at when the window closes.

**Why.** This follows an accepted precedent rather than inventing one.
`production-acceptance-v1` ships `UNKNOWN`, `NOT_RUN`, `BLOCKED`, and
`NOT_APPLICABLE` alongside `PASS` / `FAIL`, and forbids a collapsed boolean by
schema (`"not": {"required": ["supported"]}`). The project has already decided
that a status which cannot distinguish "checked and fine" from "never checked"
is not an acceptance signal. The modeling gate should not re-litigate that.

**Cost.** One additive field plus a shim. **Reversibility.** Additive, with an
explicit migration window. **Dependency.** D1 for the `unavailable` signal;
D3 can ship earlier with key-absence detection alone.

### D4 — Failure semantics: reject malformed input, never block on a violation — GO

| Site | Condition | Semantics |
| --- | --- | --- |
| Tool call arguments | Do not match `input_schema` | **Reject.** Unchanged; the dispatcher already does this. |
| Spec document | Fails `model-spec@1` | **Reject** the gate call with an actionable error. Never return a verdict derived from a spec that could not be parsed. |
| Scene export | Fails `model-state@1` | **Reject.** A malformed export is an adapter bug, not a modeling defect. |
| Scene export | Declares `partial: true`, or a check is in `unavailable` | **Degrade** the affected check or checks to `unknown`; overall `unknown`. |
| Scene vs spec | A check is violated | **Report** `fail`. Record it. Do not block the mutation. |
| Write verb declaration | No paired state export or gate tool | **Reject in CI** via the declaration lint. |

**The gate does not block mutation, ever.** Two reasons.

Mechanically, the gate runs on a state export — that is, *after* the geometry
exists. Blocking at that point cannot prevent building; it can only prevent
shipping. A check that prevents building would have to run on the arguments,
before execution, and at that point there is nothing to compare against: a
modeling spec describes a result, not a call signature.

Economically, the escape hatch already exists and is heavily used. An agent
blocked by a gate it cannot satisfy does not stop — it drops to raw Python,
which already carries 25%-87% of calls. A blocking gate therefore does not
raise quality; it lowers the typed layer's share of traffic, which is one of
the two metrics #2263 says are already failing. A gate that reports gets
adopted; a gate that blocks gets routed around.

### D5 — Who enforces: lint in core, self-check in the adapter, not the gateway — GO / GO / NO-GO

**Core, declaration lint — GO.** Extend the existing `find_unpaired_write_verbs`
rule so a modeling write verb must also declare the state export and the gate
tool it is checked by. Runtime cost zero; false-block risk zero; the
misjudgment cost lands on the adapter author at PR time, which is the cheapest
possible place to pay it. Owner: core.

**Adapter, self-check on the mutation path — GO.** The adapter calls the gate in
its own `postcondition` and reports `verified` per ADR-022. The adapter owns
host semantics — what "has UVs" means differs between Houdini and Maya — so the
adapter must own the verdict, and therefore owns the cost of a wrong one.

**Gateway or dispatch interposition that blocks mutation — NO-GO.** The gateway
holds no modeling spec and no scene state. Making it able to block requires it
to own a per-session spec plus live scene state: a new stateful service on the
hot path of every call, in every DCC. High cost, all-adapter blast radius, and
not reversible inside one migration window.

**Re-evaluation trigger.** Reconsider only if the declaration lint has shipped
for two consecutive releases and adapters still merge unpaired modeling write
verbs, i.e. the cheap layer was tried and did not take.

### D6 — Enforce `output_schema` on results — NO-GO in general, GO narrowly

`output_schema` is declared everywhere and enforced nowhere. Closing that gap
entirely is the wrong fix.

**Narrow GO.** Enforce `output_schema` only for tools whose result *is* a
versioned state export — the `dcc-mcp/*@1` family. Those are the documents other
code consumes structurally, and silent shape drift in them is the class behind
the evaluation's response-desync findings. Adoption is opt-in per tool: a tool
that does not declare a state export is untouched, and one that does declare one
is rejected when the declaration is absent or the result fails it. Validation
runs at the same site input is validated today.

**Current width: zero.** No tool in this tree is in the enforced set today —
the four existing schema names appear only in `verification/schemas.py`, its
re-exports, and tests, and no `skills/*/tools.yaml` declares `output_schema`.
This decision is therefore sequenced last: it has no effect until D1 has landed
and at least one adapter publishes a `dcc-mcp/model-state@1` export.

**Dependency.** D1, plus at least one tool declaring a versioned state export.
Land after D1 and D5.

**NO-GO for all tools.** Mutation results are heterogeneous by design and
ADR-022's envelope (`success`, `message`, `error`, `postcondition`, `_meta`) is
the correct contract for them. Requiring a bespoke output schema per tool would
multiply schemas with no consumer and break on the first additive field.

**Reversibility.** Opt-in flag; off is one deletion.

### D7 — The prompt-to-spec converter ("model X" to structured spec) — NO-GO in this slice

The roadmap bundles the converter with the gate. It should not be.

**Why not.** It is not a schema problem. It has a hard dependency on D2 landing
first. It changes what agents are told to do, so it is not reversible by
deletion once skills ship. And a weak converter feeding a faithful gate
produces the worst combination available: garbage in, enforced garbage, with
the gate's authority laundering the converter's error.

**Re-evaluation trigger.** All three of: at least ten hand-authored
`model-spec@1` documents exist across two or more adapters; the D5 lint is
green for those adapters; and at least one recorded case shows an agent
producing a spec the gate rejects for a converter-fixable reason.

### D8 — VLM soft-rescue over the gate — NO-GO here, out of scope

Belongs to the companion roadmap RFC, not this one. The boundary is recorded
here so the two documents cannot contradict each other: deterministic checks
run first and a hard-gate failure never consults a model; a model may only
rescue near-threshold soft rejects, and never a check whose status is
`unknown`. It cannot be specified at all until the deterministic verdict
vocabulary of D3 exists.

## Cost of not doing it

Without a gate, a modeling defect (missing UVs, unbound material, missing part)
survives every machine stage:

1. **At the tool call** — nowhere. The tool returns success; that is the
   seven-defect silent-success class.
2. **During the build** — nowhere. Nothing compares the scene to the spec.
3. **At export** — only if the adapter happens to check. It does not; the
   evaluator is never invoked automatically.
4. **At lookdev and texture binding** — a human notices the PBR bind is wrong
   or a turntable renders black.
5. **At compositing** — a human reads the comparison sheet.

The first deterministic checkpoint is therefore step 5: after all four hosts
have built the asset, after a licensed render, and after compositing. And it
attributes the defect to an image, not to the tool call that produced it —
which is why the 66 defects had to be filed by hand. Two costs compound:
**detection latency of an entire multi-DCC pass**, and **loss of attribution**.
The checkpoint that finally catches the class is also the weakest one, since
screenshot review cannot verify behavior. The 31%-of-meshes-without-UV figure
is the size of the class that currently reaches step 4 unchallenged.

## First step

The minimum dispatchable slice, in priority order. Step 1 is deliberately
small enough to land in one PR with no adapter or CI change.

**Step 1 — the verdict and its input contract (D2, then D3).**

- New file `python/dcc_mcp_core/schemas/model-spec-v1.schema.json`.
- `python/dcc_mcp_core/verification/scene_spec.py`: add `validate_spec_document()`;
  give `SceneSpecResult` a `status` and give each entry in `checks` one; keep
  `passed` as a derived alias for one migration window.
- `python/dcc_mcp_core/skills/verification/scripts/validate_scene_vs_spec.py`:
  reject a spec that fails `model-spec@1` with an actionable error instead of
  evaluating a default.
- `tests/test_verification_scene_spec.py`: cover malformed spec, empty scene
  reported as `unknown` rather than passed, and the `passed` alias.

**Step 2 — the state vocabulary (D1).**

- New file `python/dcc_mcp_core/schemas/model-state-v1.schema.json`.
- `python/dcc_mcp_core/verification/schemas.py`: `SCHEMA_MODEL_STATE` constant,
  `SCHEMA_VERSIONS` and `_SCHEMA_FILENAMES` entries, `_validate_model_state`,
  and the `_VALIDATORS` entry.
- `python/dcc_mcp_core/verification/scene_spec.py`: accept a `model-state@1`
  export and treat `unavailable` entries as per-check `unknown`.

**Step 3 — adoption (D5).**

- `python/dcc_mcp_core/verification/lint.py`: extend the pairing rule so a
  modeling write verb must also declare its gate tool.
- Wire the lint into the skill linter so it runs in CI. Until it runs, D5 is a
  library function with no effect, which is the current state of the pairing
  rule and the reason this step is third rather than first.

## Non-goals

No new validation framework (the existing packaging and validator helpers are
reused). No new result envelope (ADR-022 owns that). No new skill surface —
`verification__validate_scene_vs_spec` stays the entry point. No change to the
Python 3.7 compatibility posture: everything proposed is stdlib-only Python,
matching the constraint the existing schema modules already honour.

## Alternatives considered

**A blocking pre-execution gate.** Rejected: nothing to compare against before
execution, and blocking drives traffic to raw Python (D4).

**Enforce the gate in the gateway.** Rejected: requires session-scoped spec and
scene state on the hot path (D5).

**One combined model + spec schema.** Rejected: they have different authors,
different lifetimes, and different versioning pressures. The spec is written by
a human or an agent per task; the state export is produced by an adapter per
call. Coupling them forces a version bump on one whenever the other moves.

**Ship the converter first so agents can produce specs immediately.** Rejected:
it would define `model-spec@1` by example, from generated output, before any
hand-authored instance exists to validate the shape against (D7).

## Open questions

1. Does `unknown` need to be distinguishable at the report level from
   `not_run` (a check the spec did not request)? The three-state verdict can
   express both as `unknown`; splitting them is a follow-up if consumers need
   it.
2. Should the UV-coverage floor (currently 0.9, from the evaluation's 31%
   figure) be a property of the spec or a per-project default? Spec, in this
   RFC's framing — but a project-level override may be worth adding once two
   adapters disagree.
