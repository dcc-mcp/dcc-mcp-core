"""validate — Run validation suite on a skill directory."""
from __future__ import annotations

import os
import re
from typing import Any


def _read_frontmatter(skill_md_path: str) -> tuple[dict[str, Any] | None, list[str]]:
    """Parse YAML frontmatter from SKILL.md."""
    errors: list[str] = []
    try:
        with open(skill_md_path, "r", encoding="utf-8") as f:
            content = f.read()
    except Exception as e:
        return None, ["Cannot read SKILL.md: {}".format(e)]

    if not content.startswith("---"):
        return None, ["SKILL.md does not start with YAML frontmatter (---)"]

    parts = content.split("---", 2)
    if len(parts) < 3:
        return None, ["SKILL.md frontmatter is not properly closed with ---"]

    frontmatter_text = parts[1].strip()
    fm: dict[str, Any] = {}
    current_key = None
    for line in frontmatter_text.split("\n"):
        if not line.strip() or line.strip().startswith("#"):
            continue
        # Simple key: value parsing
        match = re.match(r'^(\w[\w-]*)\s*:\s*(.*)', line)
        if match:
            current_key = match.group(1)
            value = match.group(2).strip().strip('"').strip("'")
            fm[current_key] = value
        elif current_key and line.strip().startswith("- "):
            # List item
            pass

    return fm, errors


def validate(skill_path: str, strict: bool = False) -> dict[str, Any]:
    """Validate a skill directory.

    Args:
        skill_path: Path to skill directory.
        strict: Treat warnings as errors.

    Returns:
        Validation report.
    """
    errors: list[dict[str, str]] = []
    warnings: list[dict[str, str]] = []

    # Check directory exists
    if not os.path.isdir(skill_path):
        return {
            "success": False,
            "valid": False,
            "errors": [{"severity": "error", "category": "path", "message": "Directory not found: {}".format(skill_path)}],
            "warnings": [],
            "summary": "Skill directory does not exist.",
        }

    dir_name = os.path.basename(os.path.abspath(skill_path))

    # Check SKILL.md
    skill_md_path = os.path.join(skill_path, "SKILL.md")
    if not os.path.isfile(skill_md_path):
        errors.append({"severity": "error", "category": "missing_file", "message": "SKILL.md not found."})
        return {
            "success": True,
            "valid": False,
            "errors": errors,
            "warnings": warnings,
            "summary": "SKILL.md is missing.",
        }

    fm, fm_errors = _read_frontmatter(skill_md_path)
    for err in fm_errors:
        errors.append({"severity": "error", "category": "frontmatter", "message": err})

    if fm:
        # Check name
        name = fm.get("name", "")
        if not name:
            errors.append({"severity": "error", "category": "frontmatter", "message": "Missing required field: name"})
        elif not re.match(r'^[a-z0-9]+(-[a-z0-9]+)*$', name):
            errors.append({"severity": "error", "category": "frontmatter",
                          "message": "name '{}' is not valid kebab-case (max 64 chars)".format(name)})
        elif name != dir_name:
            errors.append({"severity": "error", "category": "frontmatter",
                          "message": "name '{}' does not match directory '{}'".format(name, dir_name)})

        # Check description
        desc = fm.get("description", "")
        if not desc:
            errors.append({"severity": "error", "category": "frontmatter", "message": "Missing required field: description"})
        elif len(desc) > 1024:
            errors.append({"severity": "error", "category": "frontmatter",
                          "message": "description is {} chars (max 1024)".format(len(desc))})

        # Check top-level version (should be under metadata)
        if "version" in fm:
            errors.append({
                "severity": "error", "category": "frontmatter",
                "message": "Top-level 'version' key is rejected — put version under metadata.dcc-mcp.version",
            })

        # Check metadata
        metadata = fm.get("metadata", "")
        if metadata:
            warnings.append({"severity": "warning", "category": "frontmatter",
                           "message": "metadata appears as flat key; DCC-MCP expects nested YAML block under metadata.dcc-mcp.*"})

    # Check tools.yaml if referenced
    if fm and fm.get("tools"):
        tools_path = os.path.join(skill_path, fm["tools"])
        if os.path.isfile(tools_path):
            try:
                # Very basic YAML parsing — validate structure
                with open(tools_path, "r", encoding="utf-8") as f:
                    tools_content = f.read()
                if "tools:" not in tools_content:
                    errors.append({"severity": "error", "category": "tools",
                                  "message": "tools.yaml missing 'tools:' key"})
                else:
                    tool_names = re.findall(r'^\s+-\s+name:\s+(\S+)', tools_content, re.MULTILINE)
                    if not tool_names:
                        errors.append({"severity": "error", "category": "tools",
                                      "message": "tools.yaml has no tool definitions"})
                    # Check for duplicate names
                    seen = set()
                    for tn in tool_names:
                        if tn in seen:
                            errors.append({"severity": "error", "category": "tools",
                                          "message": "Duplicate tool name: {}".format(tn)})
                        seen.add(tn)
                    # Check source_file references
                    source_files = re.findall(r'source_file:\s+(\S+)', tools_content)
                    for sf in source_files:
                        script_path = os.path.join(skill_path, sf)
                        if not os.path.isfile(script_path):
                            errors.append({"severity": "error", "category": "tools",
                                          "message": "source_file '{}' not found".format(sf)})
            except Exception:
                warnings.append({"severity": "warning", "category": "tools",
                               "message": "Could not parse tools.yaml"})
        else:
            warnings.append({"severity": "warning", "category": "tools",
                           "message": "tools.yaml referenced but not found at '{}'".format(fm["tools"])})

    # Check scripts directory
    scripts_dir = os.path.join(skill_path, "scripts")
    if os.path.isdir(scripts_dir):
        for script_file in os.listdir(scripts_dir):
            if script_file.endswith(".py") and not script_file.startswith("_"):
                script_path = os.path.join(scripts_dir, script_file)
                try:
                    with open(script_path, "r", encoding="utf-8") as f:
                        script_content = f.read()
                    # Check for avoidable imports
                    if "import requests" in script_content and "import requests." not in script_content:
                        warnings.append({
                            "severity": "warning", "category": "skill-helper-adoption",
                            "message": "scripts/{} imports 'requests' — consider dcc_mcp_core.skills_helper".format(script_file),
                        })
                    if "import yaml" in script_content:
                        warnings.append({
                            "severity": "warning", "category": "skill-helper-adoption",
                            "message": "scripts/{} imports 'yaml' — consider dcc_mcp_core.skills_helper".format(script_file),
                        })
                except Exception:
                    pass

    # Determine validity
    has_errors = len(errors) > 0
    if strict:
        has_errors = has_errors or len(warnings) > 0

    if has_errors:
        summary = "Validation found {} error(s) and {} warning(s).".format(len(errors), len(warnings))
    else:
        summary = "Skill is valid! {} warning(s) to consider.".format(len(warnings))

    return {
        "success": True,
        "valid": not has_errors,
        "errors": errors,
        "warnings": warnings,
        "summary": summary,
    }
