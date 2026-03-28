# Maya Pipeline — Help

## Overview

The `maya-pipeline` skill provides end-to-end project setup and USD export
for Autodesk Maya workflows.

## Available Scripts

### `setup_project.py`

Create a standardized Maya project directory:

```bash
python scripts/setup_project.py --name MyProject --root /projects
```

Creates:
```
/projects/MyProject/
├── scenes/
├── textures/
├── cache/
├── renders/
└── exports/
```

### `export_usd.py`

Export the current scene (or specified file) to USD format:

```bash
python scripts/export_usd.py --input scene.ma --output scene.usda --validate
```

When `--validate` is passed, this script calls into the `usd-tools` dependency
to run `usdchecker` on the output.

## Dependencies

This skill depends on:
- **maya-geometry** — for geometry operations
- **usd-tools** — for USD validation

These dependencies are declared in `SKILL.md` frontmatter and resolved
automatically by the skill runner.
