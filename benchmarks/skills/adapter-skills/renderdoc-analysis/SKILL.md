---
name: renderdoc-analysis
description: >-
  Domain skill — Inspect an existing RenderDoc capture and export its embedded thumbnail, Chrome
  trace, or drawcall texture resources, and analyse what the frame contains: sample a target region
  on a grid, scan it for NaN, Inf, negative, and out-of-band values, read a whole frame's structure
  and signals in one replay, snapshot everything one draw executed with, break the frame into passes
  and per-pass load, diff adjacent draws' pipeline state for redundant switches, time each pass
  from the GPU duration counter, and diff one target region between two draws or two captures for
  PSNR, absolute difference, and an approximate SSIM, and gate those measurements or a draw's
  pipeline state against thresholds as a pass/fail CI assertion. Use for offline graphics triage,
  automation artifacts, CI regression gates, and finding where a frame starts producing garbage or
  spending its time. Not for launching a capture — use renderdoc-capture. Not for why one pixel or vertex
  has the value it has — use renderdoc-debug.
license: MIT
compatibility: "RenderDoc 1.45+; dcc-mcp-core 0.20.14+; qrenderdoc beside renderdoccmd for the analysis tools"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: renderdoc
    layer: domain
    version: "0.4.0"
    search-hint: "RenderDoc inspect rdc chunks thumbnail Chrome trace graphics analysis sample region NaN Inf pixel diagnosis frame overview draw call state pipeline shader bindings render passes state changes redundant switches batching pass timing diff draws captures PSNR SSIM image comparison regression CI assertion gate assert pixels state threshold pass fail verdict ignore keys"
    tags: "renderdoc,analysis,thumbnail,timeline,pixel-diagnosis,frame-overview,draw-state,render-passes,state-changes,pass-timing,diff,psnr,ssim,image-comparison,regression,assert,ci-gate,state-assertion,graphics-debugging"
    tools: tools.yaml
    depends: "dcc-diagnostics"
---

# RenderDoc Analysis

Inspect before exporting. These tools never modify the input `.rdc`; exports require an explicit
destination path and create its parent directory when needed.

Use `export_drawcall_resources` with an exact event ID to export only texture resources that the
pixel shader reports as used at that event. The result records binding, resource name, dimensions,
format, and output file for each PNG.

Inspection reports `draw_dispatch_count`, `frame_work_count`, `present_count`, and
`frame_content_status` so a structurally readable capture with no rendering work is not mistaken
for a usable frame.

## Two capability tiers

This skill spans both RenderDoc backends, so check which one a tool needs before you call it:

| Tool | Backend | Answers |
| --- | --- | --- |
| `inspect_capture` | `renderdoccmd` | whether the file is structurally readable at all |
| `export_thumbnail`, `export_timeline`, `convert_capture` | `renderdoccmd` | artifacts written to a path you name |
| `export_drawcall_resources` | `renderdoc.pyd` | the textures one event actually used |
| `sample_pixel_region`, `diagnose_pixel_values` | `renderdoc.pyd` | what the pixels in a region contain |
| `get_frame_overview` | `renderdoc.pyd` | what the frame contains, in one replay |
| `get_draw_call_state` | `renderdoc.pyd` | what one draw executed with |
| `analyze_render_passes` | `renderdoc.pyd` | the pass structure and each pass's load, no counter needed |
| `analyze_state_changes` | `renderdoc.pyd` | which state switches between adjacent draws were avoidable |
| `get_pass_timing` | `renderdoc.pyd` + a timing counter | how long each pass took |
| `diff_draws` | `renderdoc.pyd` | how much one target changed between two draws of one capture |
| `diff_captures` | `renderdoc.pyd` | how much one target changed between two captures |
| `assert_pixels` | `renderdoc.pyd` | whether that change stays inside thresholds, as a pass/fail verdict |
| `assert_state` | `renderdoc.pyd` | whether two events' pipeline state matches, as a pass/fail verdict |

**CI assertion gate entry point:** four tools are tagged `group: verify` in `tools.yaml`. The two
`diff_*` tools measure and never raise on a mismatch; the two `assert_*` tools turn those
measurements into a verdict. Use `assert_*` when you want a gate, and `diff_*` when you want the
numbers without one.

The `renderdoccmd` baseline can convert and export, but it cannot read data back out of a capture.
Every tool that needs readback lives in the second tier and is gated on the **deep replay backend**
(`renderdoc.pyd`, reached through the Python runtime bundled with `qrenderdoc`). `get_pass_timing`
additionally needs a GPU duration counter, which is a property of the driver, not of the install.

When the backend is missing, those tools return a structured `unsupported_backend` result naming
the backend and how to enable it. When the backend is there but the driver exposes no duration
counter, `get_pass_timing` returns `unsupported_capture` with the counters that *are* available
instead of a frame of zero-duration passes. None of them raise, and none of them return empty data
silently. Call `renderdoc_inspect__replay_capabilities` first when you are unsure which tier you are
on, and `renderdoc_perf__perf_capabilities` before `get_pass_timing`.

## Working order

1. `inspect_capture` — confirm the file is readable before paying for a replay.
2. `get_frame_overview` — structure, resources, and the signals worth chasing, in one replay.
3. `analyze_render_passes` — where the frame's work clusters, and which pass owns a target.
4. `get_pass_timing` — how long each of those passes took, when the driver has a duration counter.
5. `analyze_state_changes` — which of the switches between adjacent draws were avoidable.
6. `get_draw_call_state` — everything one suspicious draw was running with.
7. `sample_pixel_region` — what the target actually contains at that event.
8. `diagnose_pixel_values` — where in the target the values stop making sense.
9. `renderdoc_debug__pixel_history` — why one pixel ended up that way.
10. `diff_draws` — did one draw change the target, and by how much.
11. `diff_captures` — did this build change the target, and by how much.
12. `assert_pixels` — is that change inside the tolerance, yes or no.
13. `assert_state` — does this event still run with the same state as that one.

## What is measured and what is estimated

Three of these tools report values that are derived rather than read, and each one says so in its
payload (`estimate`, `estimate_method`, or `estimate_fields`) as well as in its summary. Read that
before quoting a number:

- **`sample_pixel_region`** reads the texels its grid lands on. A grid coarser than the region makes
  the min, max, and mean a sample of the region, not a census of it. A grid that covers every texel
  reports `estimate: false`. Values are decoded as floats for every format, so a 64-bit integer
  channel comes back rounded.
- **`diagnose_pixel_values`** strides over a region larger than `max_texels` (two million by
  default), and the counts are then a sample of that region. The stride is reported as
  `region.step`.
- **`get_frame_overview`** and **`analyze_render_passes`** derive pass triangle counts from the API
  draw parameters (`numIndices / 3 * numInstances`), which is before culling, clipping, and vertex
  shading. Both list it in `estimate_fields`; the overview's signals are listed there too, because
  they are thresholds over the capture's structure.
- **`get_pass_timing`** reports the pass event's own counter sample when the driver sampled it, and
  the sum of the pass's timed actions when it did not. The method is reported per pass in
  `duration_method` and `derived`, so a summed number is never read as a measured one.

Two more limits are about the format, not the estimate: a block-compressed, packed, or
special-encoded texture cannot be sampled at all, and both pixel tools report that as an
unsupported result with the reason instead of sampled zeros. Likewise, NaN and Inf only exist in a
floating-point format, so `diagnose_pixel_values` reports those checks as not applicable for an
integer format, with the reason, rather than as zero anomalies.

`analyze_state_changes` reads one pipeline state per draw, so it is bounded by `max_events` (64 by
default): it looks at the first N draws of the range, not the whole frame, and reports
`event_count_truncated` when there were more.

## What the assertion gates decide

`assert_pixels` and `assert_state` are the CI gate layer over the two diff tools. Both return a
verdict instead of raising, so a failing build is a normal successful call that reports `passed:
false` and lists which check failed with its measured value. Only a backend or replay problem comes
back as an unsupported result.

`assert_pixels` delegates the measurement to `diff_draws` / `diff_captures`, so there is one
implementation of the pixel comparison and every caveat in the section below applies to it too.
Supply **at least one** of `max_mean_abs_diff`, `min_psnr`, or `max_failed_pixel_ratio` — a gate with
no criterion always passes, so an empty set is reported as unsupported rather than as green.

- `max_*` thresholds tolerate measurements **at or below** them; `min_psnr` demands a value **at or
  above** it. Every checked threshold is reported with its measured value.
- **Two identical regions satisfy any `min_psnr`.** Their PSNR is infinite and therefore reported as
  `null`, which is the best possible outcome rather than a missing measurement.
- **`evaluable: false` means no threshold was judged.** When the two sides are not comparable,
  nothing was measured, so the result reports `passed: false` with `evaluable: false` and the
  reason. That is neither a pass nor a regression — treat it as a broken gate, not a green build.
- **NaN and Inf are reported, not gated.** They are excluded from the metrics and counted under
  `non_finite`, because they are not one of the thresholds this tool was specified to enforce. Check
  `non_finite.excluded_texel_count` if a NaN should fail your build.

`assert_state` compares the same pipeline-state fingerprint `analyze_state_changes` uses, so a
difference it reports is a state switch that tool would have counted.

- **`ignore_keys` names the sections to leave out**, which is how a gate tolerates a change it
  expects. Ignored keys are listed in `ignored_keys`, so an ignored change is never invisible, and a
  key you name that is not in the fingerprint is reported in `unknown_ignore_keys` rather than
  silently accepted — that is usually a typo.
- A key present on only one side is a difference with `null` on the missing side, because a state
  section one replay exposes and the other does not is itself worth seeing.

## What the diff tools measure, and what they approximate

`diff_draws` and `diff_captures` report numbers that are easy to over-read, so each one carries its
own basis in the payload and in the summary.

- **PSNR always travels with `psnr_basis`.** It records the bit depth, the channels that took part,
  the `max_value` the ratio was computed against, and where that `max_value` came from
  (`caller_supplied` when you passed one, `format_nominal_peak` when it was derived from the
  format). Without it, two PSNR figures are not comparable. Pass `max_value` explicitly when you
  want a threshold that means the same thing on every format.
- **Two identical regions report `identical: true` and `psnr: null`.** Their PSNR is mathematically
  infinite, and an infinity compared against a threshold is a bug waiting to happen, so it is not
  reported as a number.
- **`ssim_approx` is not the standard SSIM.** It averages over 8x8 blocks with a box (uniform)
  window on a sampling grid, not over an 11x11 Gaussian window at every pixel — a full-resolution
  sliding window is not affordable in pure Python. The name, the block size, the window, and the
  grid step are all in the payload (`ssim_block_size`, `ssim_window`, `ssim_grid_step`) and in the
  summary, so its numbers are never mistaken for the standard index. It is `null` with
  `ssim_skipped_reason` when the region is smaller than one block.
- **NaN and Inf are counted, not folded in.** Any texel that is non-finite on either side is
  excluded from the MSE and counted separately under `non_finite`, including the counts where one
  side is NaN and the other is not. One NaN would otherwise make a PSNR read "identical" or
  "infinitely bad" with nothing in between.
- **A region above `max_texels` is sampled, not censused.** The stride is reported as `estimate`
  and `estimate_method`. Both sides use the same stride, so they stay comparable.
- **`failed_texel_ratio` is measured against `threshold`**, which defaults to one 8-bit code value
  (1/255). Set it to the tolerance your gate actually means.

### `diff_captures` replays twice, and either replay can fail alone

A RenderDoc replay context holds one capture at a time, so comparing two captures means two
`qrenderdoc` launches, staged through a temporary directory that is removed on every path out of
the call. Consequences worth knowing:

- Both sides write a sidecar describing their size, format, and stride. The two sidecars are
  checked against each other **before** any texel is compared. A size, format, or API mismatch
  comes back as `comparable: false` with `reason_code` and `reason` — not as an exception, and not
as a silently wrong number. Pass `force=true` to compare anyway; a forced comparison uses the
  region both sides have in common, reading each side with **its own** row stride, and says so in
  `forced_reason` and `force_applied`.
- **`force` has limits.** A different `comp_count` or a different `sample_step` is *not* something
  force can wave through: there is no cropping that makes texel `(x, y)` mean the same thing on
  both sides, so those come back as `comparable: false` with `reason_code` of
  `component_count_mismatch` or `sample_step_mismatch` and `metrics: null` even when you asked to
  force. Force is an assertion that a known difference is acceptable, not a licence to compare
  incomparable data.
- If either capture fails to replay, the result is a structured `unsupported_capture` naming the
  side that failed. **Half a replay is never turned into a diff.**
- `match_by` lines the two captures up: `event_id` for a known id, `index` for the Nth draw,
  `name` for the draw with that name. Only real draws are numbered — marker and clear events do
  not shift the index.
- `diff_draws` needs none of this: it replays once and moves the replay between the two events, so
  prefer it whenever both events live in the same capture.

Replay runs in the Python interpreter bundled with `qrenderdoc`. On headless Linux, run the adapter
under Xvfb or provide another working X/Wayland display; the official archive does not include Qt's
`offscreen` platform plugin.
