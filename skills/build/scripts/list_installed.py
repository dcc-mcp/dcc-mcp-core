"""list_installed — List locally installed DCC-MCP skills."""
from __future__ import annotations

import json
import os
import subprocess
from typing import Any


def list_installed(
    dcc_type: str | None = None,
    layer: str | None = None,
) -> dict[str, Any]:
    """List installed skills.

    Args:
        dcc_type: Filter by DCC type.
        layer: Filter by skill layer.

    Returns:
        List of installed skills.
    """
    skills: list[dict[str, str]] = []

    # Try via marketplace CLI
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "marketplace", "list", "--installed", "--output", "json"],
            capture_output=True, text=True, timeout=15,
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            raw_skills = data.get("skills", data.get("results", []))
            for s in raw_skills:
                s_layer = s.get("layer", "")
                s_dcc = s.get("dcc_type", s.get("dcc", ""))
                if layer and s_layer != layer:
                    continue
                if dcc_type and s_dcc != dcc_type:
                    continue
                skills.append({
                    "name": s.get("name", ""),
                    "version": s.get("version", ""),
                    "layer": s_layer,
                    "dcc": s_dcc,
                    "path": s.get("path", s.get("install_path", "")),
                })
    except Exception:
        pass

    # Fallback: scan skill paths
    if not skills:
        skill_paths_env = os.environ.get("DCC_MCP_SKILL_PATHS", "")
        for path_entry in skill_paths_env.split(os.pathsep) if skill_paths_env else []:
            path_entry = path_entry.strip()
            if not path_entry or not os.path.isdir(path_entry):
                continue
            for entry in os.listdir(path_entry):
                entry_path = os.path.join(path_entry, entry)
                if os.path.isdir(entry_path):
                    skill_md = os.path.join(entry_path, "SKILL.md")
                    if os.path.isfile(skill_md):
                        # Parse basic info
                        try:
                            with open(skill_md, "r", encoding="utf-8") as f:
                                content = f.read()
                            import re
                            name_match = re.search(r'^name:\s*(.+)', content, re.MULTILINE)
                            ver_match = re.search(r'version:\s*"([^"]+)"', content)
                            layer_match = re.search(r'layer:\s*(.+)', content, re.MULTILINE)
                            dcc_match = re.search(r'dcc:\s*(.+)', content, re.MULTILINE)
                            s_layer = (layer_match.group(1).strip() if layer_match else "unknown")
                            s_dcc = (dcc_match.group(1).strip() if dcc_match else "unknown")
                            if layer and s_layer != layer:
                                continue
                            if dcc_type and s_dcc != dcc_type:
                                continue
                            skills.append({
                                "name": name_match.group(1).strip() if name_match else entry,
                                "version": ver_match.group(1) if ver_match else "unknown",
                                "layer": s_layer,
                                "dcc": s_dcc,
                                "path": entry_path,
                            })
                        except Exception:
                            pass

    return {
        "success": True,
        "count": len(skills),
        "skills": skills,
    }
