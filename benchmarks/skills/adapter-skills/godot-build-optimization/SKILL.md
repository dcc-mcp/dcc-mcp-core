---
name: godot-build-optimization
description: >-
  Analyze a Godot project and generate a version-aware SCons export-template
  profile, optional .gdbuild feature profile, and reproducible artifact-size
  report. Use when reducing packaged engine size before export; keep runtime
  validation and risky binary post-processing explicit.
license: MIT
compatibility: "Godot 4.4+; dcc-mcp-core 0.19+; Python 3.9+"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: godot
    layer: domain
    version: "0.1.0"
    search-hint: "Godot build size optimization custom.py gdbuild SCons export template UPX wasm-opt Brotli"
    tags: "godot,build,optimization,export,game-development"
    skill-reference-docs:
      - "references/*.md"
---

# Godot Build Optimization

Use the bundled `scripts/optimize_build.py` helper from the repository or
installed package. It performs a bounded static scan and writes only to the
explicit output directory; it does not edit `project.godot`, export presets, or
engine source.

1. Inspect the project and target first. Run `plan` and review the detected
   renderer, text scripts, 2D/3D, navigation, XR, archive, and asset evidence:

   ```text
   python scripts/optimize_build.py plan --project <project> --target windows
   ```

2. Generate files only after the plan is appropriate for the target. Pass the
   exact Godot source version when it is known and keep output in a dedicated
   staging directory:

   ```text
   python scripts/optimize_build.py generate \
     --project <project> --target windows --godot-version 4.5 \
     --output <staging>/godot-build-size
   ```

   This creates `custom.py`, `build_config.gdbuild`, and
   `optimization-report.json`. Compile matching export templates with the
   generated files, for example:

   ```text
   scons platform=windows profile=<staging>/godot-build-size/custom.py \
     build_profile=<staging>/godot-build-size/build_config.gdbuild
   ```

3. Use `--minimal-modules` only when the project has runtime smoke coverage.
   It sets `modules_enabled_by_default=no` and enables only modules visible in
   the scan. Add `--force <feature>` when a feature is loaded dynamically or
   otherwise cannot be found statically. Treat every generated disable as a
   hypothesis until the release artifact launches cleanly.

4. Export to a clean directory with the matching custom template, then launch
   that artifact without the editor. Check the first scene, localized text,
   input, saves, native extensions, and startup logs. Measure the result and
   preserve the report and digest:

   ```text
   python scripts/optimize_build.py measure --artifact <exported-file> \
     --output <staging>/artifact-size.json
   ```

Read [Size optimization guidance](references/size-optimization.md) before
choosing fallback text, module allowlisting, UPX, Wasm-opt, or Brotli. These
choices can remove runtime features or change memory, antivirus, and hosting
behavior.
