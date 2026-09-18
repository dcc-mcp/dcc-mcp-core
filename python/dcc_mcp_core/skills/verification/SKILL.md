---
name: verification
description: >-
  Cross-DCC verification toolset - deterministic capture review views, in-band
  image statistics, scene-vs-spec validation, and reference-vs-render comparison
  sheets. Pure Python, read-only, DCC-agnostic. Use to close the quality loop
  without shipping pixels to a model: flag white/near-black/gamma-broken frames
  from stats alone and catch missing-UV/missing-part exports before export.
  Not for scoring or aesthetic review - this toolset packages evidence and
  computes no scores.
license: MIT
compatibility: Uses ffmpeg/ffprobe on PATH (or via vx) only for non-PPM image decode and comparison-sheet compositing.
metadata:
  dcc-mcp:
    dcc: python
    version: "0.1.0"
    layer: infrastructure
    search-hint: "verification, capture, image stats, luma, histogram, comparison sheet, scene spec, uv coverage, blank frame, review views"
    tags: "verification, capture, image-stats, comparison-sheet, scene-spec, uv, dcc-agnostic"
    tools: tools.yaml
---

# Verification

A cross-DCC, read-only verification toolset (`affinity: any`) that closes the
capture-and-look quality loop with deterministic, machine-checkable signals.
Inspired by img2threejs's "deterministic-first, model-last" review design, it
computes evidence in-band — statistics, hashes, and verdicts — and never ships
pixels to a model unless a caller explicitly asks for `image`/`both` payloads.

The four tools map to the four legs of the deterministic capture contract:

- `capture_review_views` — the fixed front / side / top / three-quarter view
  plan, viewport-only, fails loudly on a missing, wrong-size, or blank frame.
- `image_stats` — mean luma, stddev, histogram, uniformity, and bad-frame flags
  (white playblast, near-black render, missing display transform).
- `validate_scene_vs_spec` — hierarchy names, material bindings, per-mesh UV
  coverage, part existence, non-manifold, and Euler checks, before export.
- `make_comparison_sheet` — reference + renders side-by-side into one image;
  packages evidence and computes no scores.

Pixel-level metrics (pHash / dHash / aHash, silhouette IoU, SSIM, CIEDE2000,
Sobel edge maps) live in `dcc_mcp_core.verification` as deterministic library
functions, not skill tools, so callers gate on them directly.

## Safety Contract

Every tool is read-only and side-effect free except `make_comparison_sheet`,
which writes exactly one output image at the requested path. Tools never mutate
the input captures and never launch a DCC. PPM/PGM inputs decode through the
standard library; other formats (PNG/JPEG/EXR) decode through `ffmpeg` on
`PATH` (or `vx ffmpeg`) and fail loudly when the decoder is unavailable.
