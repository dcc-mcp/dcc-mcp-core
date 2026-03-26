"""Skill loader for parsing SKILL.md and registering script actions.

This module provides functions to parse OpenClaw-compatible SKILL.md files,
enumerate scripts in the scripts/ directory, and register them as Actions
in the ActionRegistry.
"""

# Import built-in modules
import logging
import os
import re
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Type

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.constants import SKILL_METADATA_FILE
from dcc_mcp_core.constants import SKILL_SCRIPTS_DIR
from dcc_mcp_core.constants import SUPPORTED_SCRIPT_EXTENSIONS
from dcc_mcp_core.models import SkillMetadata
from dcc_mcp_core.skills.script_action import create_script_action

logger = logging.getLogger(__name__)

# Pattern to match YAML frontmatter delimited by ---
_FRONTMATTER_PATTERN = re.compile(r"^---\s*\n(.*?)\n---\s*\n", re.DOTALL)

# Pattern to extract key: value pairs from YAML (simple subset)
_YAML_KV_PATTERN = re.compile(r'^(\w+)\s*:\s*(.+)$', re.MULTILINE)

# Pattern to extract YAML list items
_YAML_LIST_PATTERN = re.compile(r'^\s*-\s*"?([^"]*)"?\s*$', re.MULTILINE)


def _try_yaml_parse(frontmatter_text: str) -> Optional[Dict[str, Any]]:
    """Try to parse YAML frontmatter using PyYAML if available.

    Falls back to None if PyYAML is not installed.

    Args:
        frontmatter_text: Raw YAML text from frontmatter.

    Returns:
        Parsed dictionary or None if PyYAML is unavailable.

    """
    try:
        import yaml  # type: ignore[import-untyped]
        return yaml.safe_load(frontmatter_text)
    except ImportError:
        return None
    except Exception as e:
        logger.debug(f"PyYAML parse failed, falling back to regex: {e}")
        return None


def _regex_parse_frontmatter(frontmatter_text: str) -> Dict[str, Any]:
    """Parse YAML frontmatter using simple regex extraction.

    Supports basic key: value and key: [list] patterns.
    This is a fallback for environments without PyYAML (e.g., Maya's embedded Python).

    Args:
        frontmatter_text: Raw YAML text from frontmatter.

    Returns:
        Dictionary of parsed key-value pairs.

    """
    result: Dict[str, Any] = {}

    for match in _YAML_KV_PATTERN.finditer(frontmatter_text):
        key = match.group(1).strip()
        value = match.group(2).strip()

        # Handle inline list: ["item1", "item2"]
        if value.startswith("[") and value.endswith("]"):
            inner = value[1:-1]
            items = [item.strip().strip('"').strip("'") for item in inner.split(",") if item.strip()]
            result[key] = items
        # Handle quoted string
        elif (value.startswith('"') and value.endswith('"')) or (value.startswith("'") and value.endswith("'")):
            result[key] = value[1:-1]
        # Handle boolean
        elif value.lower() in ("true", "false"):
            result[key] = value.lower() == "true"
        else:
            result[key] = value

    return result


def parse_skill_md(skill_dir: str) -> Optional[SkillMetadata]:
    """Parse a SKILL.md file from a skill directory.

    Extracts the YAML frontmatter (name, description, tools, etc.)
    and enumerates scripts in the scripts/ subdirectory.

    Args:
        skill_dir: Absolute path to the skill directory containing SKILL.md.

    Returns:
        SkillMetadata if parsing succeeds, None otherwise.

    """
    skill_md_path = os.path.join(skill_dir, SKILL_METADATA_FILE)

    if not os.path.isfile(skill_md_path):
        logger.warning(f"SKILL.md not found at: {skill_md_path}")
        return None

    try:
        with open(skill_md_path, "r", encoding="utf-8") as f:
            content = f.read()
    except (OSError, UnicodeDecodeError) as e:
        logger.warning(f"Error reading {skill_md_path}: {e}")
        return None

    # Extract frontmatter
    fm_match = _FRONTMATTER_PATTERN.match(content)
    if not fm_match:
        logger.warning(f"No YAML frontmatter found in: {skill_md_path}")
        return None

    frontmatter_text = fm_match.group(1)

    # Try PyYAML first, fallback to regex
    parsed = _try_yaml_parse(frontmatter_text)
    if parsed is None:
        parsed = _regex_parse_frontmatter(frontmatter_text)

    if not parsed or not isinstance(parsed, dict):
        logger.warning(f"Failed to parse frontmatter in: {skill_md_path}")
        return None

    # Ensure 'name' field exists, fallback to directory name
    if "name" not in parsed or not parsed["name"]:
        parsed["name"] = os.path.basename(skill_dir)

    # Enumerate scripts
    scripts = _enumerate_scripts(skill_dir)
    parsed["scripts"] = scripts
    parsed["skill_path"] = os.path.abspath(skill_dir)

    # Ensure list fields
    for list_field in ("tools", "tags", "scripts"):
        if list_field in parsed and not isinstance(parsed[list_field], list):
            parsed[list_field] = [parsed[list_field]]

    try:
        return SkillMetadata(**parsed)
    except Exception as e:
        logger.warning(f"Error creating SkillMetadata from {skill_md_path}: {e}")
        return None


def _enumerate_scripts(skill_dir: str) -> List[str]:
    """Enumerate script files in the scripts/ subdirectory.

    Args:
        skill_dir: Path to the skill directory.

    Returns:
        List of absolute paths to supported script files.

    """
    scripts_dir = os.path.join(skill_dir, SKILL_SCRIPTS_DIR)
    if not os.path.isdir(scripts_dir):
        return []

    scripts = []
    try:
        with os.scandir(scripts_dir) as entries:
            for entry in entries:
                if not entry.is_file(follow_symlinks=True):
                    continue
                ext = os.path.splitext(entry.name)[1].lower()
                if ext in SUPPORTED_SCRIPT_EXTENSIONS:
                    scripts.append(os.path.abspath(entry.path))
    except OSError as e:
        logger.warning(f"Error scanning scripts directory {scripts_dir}: {e}")

    scripts.sort()
    return scripts


def load_skill(
    skill_dir: str,
    registry: Optional[ActionRegistry] = None,
    dcc_name: Optional[str] = None,
) -> List[Type[Action]]:
    """Load a Skill package and register its scripts as Actions.

    Parses the SKILL.md, creates ScriptAction classes for each script,
    and registers them in the ActionRegistry.

    Args:
        skill_dir: Path to the skill directory containing SKILL.md.
        registry: ActionRegistry to register actions in. Uses singleton if None.
        dcc_name: Optional DCC name override for the generated actions.

    Returns:
        List of registered Action classes.

    """
    metadata = parse_skill_md(skill_dir)
    if metadata is None:
        return []

    if registry is None:
        registry = ActionRegistry()

    effective_dcc = dcc_name or metadata.dcc
    registered_actions: List[Type[Action]] = []

    for script_path in metadata.scripts:
        try:
            action_class = create_script_action(
                skill_name=metadata.name,
                script_path=script_path,
                skill_metadata=metadata,
                dcc_name=effective_dcc,
            )
            if registry.register(action_class):
                registered_actions.append(action_class)
                logger.debug(f"Registered script action: {action_class.name} from {script_path}")
            else:
                logger.debug(f"Skipped registration for: {script_path}")
        except Exception as e:
            logger.warning(f"Error creating script action for {script_path}: {e}")

    if registered_actions:
        logger.info(
            f"Loaded skill '{metadata.name}' with {len(registered_actions)} action(s) "
            f"from {skill_dir}"
        )

    return registered_actions
