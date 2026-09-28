# Skills benchmark dataset

Seed corpus for [`crates/dcc-mcp-skills-bench`](../../crates/dcc-mcp-skills-bench).

## What is in here

| file | what it holds |
|---|---|
| `seeds.json` | real skills harvested from **this** repository |
| `adapters.json` | real skills harvested from seven pinned DCC adapter repositories |

Both are parsed with the production loader (`dcc_mcp_skills::parse_skill_md`),
so every entry carries the same fields the ranker sees at runtime: SKILL.md
frontmatter plus the sibling `tools.yaml` tool declarations.

### `seeds.json`

Harvested from these workspace-relative roots:

| root | why |
|---|---|
| `skills/` | skills this repository ships |
| `examples/skills/` | worked examples, including edge cases |
| `python/dcc_mcp_core/skills/` | bundled Python-side skills |
| `tests/fixtures/skills/` | loader edge cases, useful as ranking stress cases |

### `adapters.json`

Harvested from the roots listed in
[`adapters::adapter_sources`](../../crates/dcc-mcp-skills-bench/src/adapters.rs),
each pinned to a full commit SHA. See that module for why the snapshot is
committed rather than cloned at benchmark time, and for the refresh command.

## Why there are two files

`seeds.json` is a live harvest: `committed_snapshot_matches_a_live_harvest`
fails whenever a SKILL.md in the four roots above changes, so a shipped skill
cannot silently drift out of the benchmark. That makes it the right gate for
this repository's own skills.

`adapters.json` cannot work that way — it is harvested from other repositories,
so a live re-harvest would make this repository's CI depend on seven
repositories' HEADs and on the network. It is instead pinned by commit and
refreshed deliberately.

## Regenerating

```bash
# This repository's own skills — run whenever a SKILL.md changes.
cargo run -p dcc-mcp-skills-bench --bin skills-bench -- regenerate-seeds

# The pinned adapter catalogues — needs network and git; run only on purpose.
cargo run -p dcc-mcp-skills-bench --bin skills-bench -- harvest-adapters
```

## Versioning

The dataset and the bench crate evolve in the same commit — there is no pinned
core version and no external repository to keep in sync. `schema` in both files
carries `CORPUS_SCHEMA_VERSION`; a snapshot whose schema does not match the
generator is treated as absent and regenerated (or ignored, for the adapter
snapshot) rather than used.

## Recall context coverage

The benchmark reports `RecallContext` coverage over the real seed pool — the
share of the `app_type` / `domain` / `workflow_stage` / `task_category` slots
that are actually populated. It is a reported measurement, not a hit-rate gate:
it says how much structured signal discovery has to work with.

The gate that *is* enforced lives in
[`recall::tests::shipped_core_skills_carry_recall_context`](../../crates/dcc-mcp-skills-bench/src/recall.rs)
and covers the skills **this repository ships** (≥ 90% of field slots). Adapter
repositories are measured and reported but cannot be fixed from here; their
`SKILL.md` frontmatter lives in their own repositories.

Note that `recall-context` was authored in SKILL.md long before anything read
it: before PIP-3701 the loader dropped the key, so coverage was 0% even on
skills that declared all four fields. Measuring the field and parsing it had to
land together.

## Trend series

The weekly **schedule** records one point per run across all three dimensions
and renders them together as `skills-bench-trend.md` — the place to do a
periodic review without opening three different workflow runs.

**One point means one week.** The `latency-trend` job also runs on every push
to main (that is what keeps the criterion bench from rotting), but only the
scheduled run records a point. Recording on every push made the window a
function of how much the team shipped: at ~7 main pushes a day, four points
spanned about half a day instead of a month, and a sustained regression was
absorbed into its own baseline within four pushes. `BASELINE_WINDOW = 4` is
four **weeks** and `MAX_POINTS = 52` is one **year** of weekly runs; both are
only true under weekly recording. A regression that took two months to arrive
is readable against its own baseline only because of that.

| dimension | contract |
|---|---|
| hit rate | merge gate; the trend shows whether the margin is being eaten |
| context growth | merge gate; the trend shows the headroom under the cap |
| latency | **no gate** — compared against a rolling median and reported |

Latency is compared against the **median p95 of the last four runs on the same
corpus**, and alerts at +50%. It is deliberately not a merge gate: CI hardware
alone can move the number by a factor of two, so a fixed cap here produces
false failures instead of catching regressions. An alert opens an issue; it
never fails a build.

Because the window is four weeks, a regression that arrives is **reported once
and then absorbed**: the point after it is measured against a baseline that
includes it. The ratchet is the point of a rolling baseline, not a defect —
the first alert is the signal, and it opens an issue that nothing closes
automatically.

### Calibrating the +50%

+50% is a starting point, not a measured threshold. The quantity it guards is
the noisiest statistic available here: `run::grade` takes **one wall-clock
sample per query, with no warm-up**, and `p95` is a nearest-rank tail value.
On one machine running the same code, `dcc_filtered` p95 has been observed
between 1417us and 4274us — roughly 3x. The rolling median takes that spread
out of the denominator; the current run's own draw is still in the numerator.

The trend report therefore prints the baseline **min–max band** next to the
median. Once a full window has accumulated:

- band narrower than +50% → the threshold is outside the noise, leave it alone
- band wider than +50% → alerts are noise; widen `LATENCY_REGRESSION_RATIO` in
  `crates/dcc-mcp-skills-bench/src/trend.rs` before trusting them

**This cannot be done before the window fills.** One point is one week, so the
first full window lands four weeks after the current corpus epoch opened, and
recalibrating earlier would be reading a band that does not exist yet.

### Who is told, and how

An alert opens (or appends to) one GitHub issue labelled
`skills-bench-latency-regression` and links the run. GitHub Actions cannot by
itself mention or assign a person, so the human half of the contract is a
**subscription, not a step**: anyone responsible for latency triage has to be
watching that label, and the issue has to be assigned to whoever owns the
triage.

Runbook for an alert:

1. Open the linked run and read the `Latency` table in the job summary
   (current p95, baseline median, **baseline min–max**, delta).
2. If the delta is inside the min–max band, it is noise: comment on the issue,
   widen `LATENCY_REGRESSION_RATIO`, and close it.
3. If it is outside the band, treat it as a regression: assign the issue to the
   owner of the ranking/discovery path and triage against the commit named in
   the trend report.
4. Either way the issue is **not** a build failure. Nothing was blocked; the
   alert exists only so that someone finds out.

To rehearse the path without waiting for a real regression, run the workflow
manually (`workflow_dispatch`) with `exercise_alert_path` enabled: it walks
issue creation with a synthetic alert and says so in the body. The rehearsal
writes its own payload rather than reading one, so it works on a run that
recorded no point.

### Corpus epochs

Every point carries a corpus fingerprint — `CORPUS_SCHEMA_VERSION`, the seed
count, and two digests:

| digest | covers |
|---|---|
| names | every skill name in the catalogue |
| query inputs | the query targets, the hard-negative pairs, and the indexed vocabulary (description, search hint, tags, tool text) |

Points whose fingerprint differs are excluded from the baseline window. The
second digest is the one that is easy to miss: query generation reads far more
of a skill than its name, so editing a `SKILL.md` description changes what was
measured while leaving every name — and the name digest — untouched. Before it
was added, the only way to open an epoch for such a change was to remember to
bump `CORPUS_SCHEMA_VERSION` by hand.

That is what keeps the series honest when the corpus changes: expanding the
seed set opens a new epoch with an empty window, so alerts stay disarmed until
four runs have accumulated under the new corpus. **Expect several weeks of
"warming up" after any corpus change — that is the mechanism working, not a
defect.**

### Where the series lives

`trend-history.json` is gitignored and carried between runs as a workflow
artifact. It is derived data, and committing it weekly would feed
release-please as code activity.

The restore step cannot simply take the most recent successful run:
`latency-trend` is skipped on pull requests, which are the majority of runs
here, so "the last successful run" usually uploaded no trend artifact at all.
It walks back through the recent successful runs and takes the first one that
carries `skills-bench-trend-history`. If the log says
`no recent scheduled run on main carries the ... artifact` twice in a row, the
series is not being carried and the baseline window will never fill.

Three details keep that walk honest, and they are load-bearing together with
weekly recording rather than independently:

- **The scan is scoped** to `status=success&event=schedule&branch=main`. With
  only the weekly run recording a point, an unscoped `per_page=20` would sit
  behind a week of pushes and dispatches and never reach a run that carries
  the artifact — the series would restart every week. `branch=main` also stops
  a `workflow_dispatch` against a feature branch from seeding the main
  baseline with another branch's numbers.
- **A name match is not proof the series is inside.** A crashed run still
  uploads the artifact (`if: always()`), so the restore downloads before it
  believes the name and keeps walking back past an empty one. Adopting an
  empty artifact is how a series silently restarts from a single point.
- **The upload is `if-no-files-found: error`.** Publishing an empty artifact
  under this name poisons the scan for every later run, so the step fails
  instead. That also marks the run failed, which keeps it out of the
  `status=success` scan.

### One writer

`latency-trend` carries its own `concurrency` group, unkeyed by ref. The
workflow-level group is keyed on `github.ref`, so a dispatch against another
branch used to run alongside a main push: both restored the same predecessor,
both appended, and the later upload dropped the other's point.
`cancel-in-progress: false` — a run that is about to upload is the wrong one to
cancel.

## When this moves out

The issue that created this benchmark set a split condition: extract the
dataset into its own repository once it exceeds 10 MB, or once a real
cross-adapter device matrix is needed. The bench crate stays in
`dcc-mcp-core` either way — it has to, because it drives
`dcc-mcp-gateway-search` through its Rust API rather than over HTTP.

Current size: **3.0 MB** (`adapters.json` 2.8 MB, `seeds.json` 161 kB), so the
10 MB condition is not met and nothing is split. Re-check with
`du -sh benchmarks/skills` whenever the adapter snapshot is refreshed.
