"""Keep PR discovery coverage aligned with the official seed harvester."""

from pathlib import Path
import re

import yaml


def test_benchmark_pr_paths_cover_every_seed_root():
    root = Path(__file__).parents[1]
    source = (root / "crates/dcc-mcp-skills-bench/src/seeds.rs").read_text(encoding="utf-8")
    block = re.search(r"pub const SEED_ROOTS:.*?= \[(.*?)\];", source, re.S).group(1)
    seed_roots = re.findall(r'"([^"]+)"', block)
    assert len(seed_roots) == 4
    workflow = yaml.safe_load((root / ".github/workflows/skills-bench.yml").read_text(encoding="utf-8"))
    # PyYAML's YAML 1.1 loader interprets the GitHub Actions `on` key as True.
    events = workflow.get("on", workflow.get(True))
    paths = events["pull_request"]["paths"]
    assert all(seed_root + "/**" in paths for seed_root in seed_roots)
