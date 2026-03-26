"""Skill scanner for discovering SKILL.md files in directories.

This module provides the SkillScanner class and utility functions for
scanning directories to find OpenClaw-compatible Skill packages
(directories containing a SKILL.md file).
"""

# Import built-in modules
import logging
import os
from typing import Dict
from typing import List
from typing import Optional

# Import local modules
from dcc_mcp_core.constants import SKILL_METADATA_FILE
from dcc_mcp_core.utils.filesystem import get_skill_paths_from_env
from dcc_mcp_core.utils.filesystem import get_skills_dir

logger = logging.getLogger(__name__)


class SkillScanner:
    """Scanner for discovering Skill packages in directories.

    Scans configured paths for directories containing SKILL.md files,
    caches results using file modification times to avoid redundant IO.

    Attributes:
        _cache: Dictionary mapping skill directory paths to their mtime.
        _skill_dirs: List of discovered skill directory paths.

    """

    def __init__(self) -> None:
        """Initialize the SkillScanner."""
        self._cache: Dict[str, float] = {}
        self._skill_dirs: List[str] = []

    def scan(
        self,
        extra_paths: Optional[List[str]] = None,
        dcc_name: Optional[str] = None,
        force_refresh: bool = False,
    ) -> List[str]:
        """Scan all configured paths for Skill packages.

        Searches in the following order (higher priority first):
        1. Explicitly provided extra_paths
        2. DCC_MCP_SKILL_PATHS environment variable
        3. Platform-specific skills directory (optionally DCC-scoped)

        Args:
            extra_paths: Additional paths to scan for skills.
            dcc_name: Optional DCC name for DCC-specific skills directory.
            force_refresh: If True, ignore cache and rescan all paths.

        Returns:
            List of absolute paths to directories containing SKILL.md.

        """
        search_paths: List[str] = []

        # 1. Extra paths (highest priority)
        if extra_paths:
            search_paths.extend(extra_paths)

        # 2. Environment variable paths
        env_paths = get_skill_paths_from_env()
        search_paths.extend(env_paths)

        # 3. Platform-specific skills directory
        try:
            platform_dir = get_skills_dir(dcc_name=dcc_name, ensure_exists=False)
            if os.path.isdir(platform_dir):
                search_paths.append(platform_dir)
        except Exception:
            pass

        # Also check global skills dir if dcc_name was specified
        if dcc_name:
            try:
                global_dir = get_skills_dir(dcc_name=None, ensure_exists=False)
                if os.path.isdir(global_dir):
                    search_paths.append(global_dir)
            except Exception:
                pass

        # Deduplicate while preserving order
        seen = set()
        unique_paths = []
        for p in search_paths:
            abs_p = os.path.abspath(p)
            if abs_p not in seen:
                seen.add(abs_p)
                unique_paths.append(abs_p)

        # Scan each path for SKILL.md
        discovered = []
        for search_path in unique_paths:
            discovered.extend(self._scan_directory(search_path, force_refresh))

        self._skill_dirs = discovered
        logger.debug(f"Discovered {len(discovered)} skill(s) across {len(unique_paths)} search path(s)")
        return discovered

    def _scan_directory(self, search_path: str, force_refresh: bool = False) -> List[str]:
        """Scan a single directory for Skill packages.

        A Skill package is a subdirectory containing a SKILL.md file.

        Args:
            search_path: Path to the directory to scan.
            force_refresh: If True, ignore cache for this path.

        Returns:
            List of absolute paths to skill directories found.

        """
        results = []
        if not os.path.isdir(search_path):
            return results

        try:
            with os.scandir(search_path) as entries:
                for entry in entries:
                    if not entry.is_dir(follow_symlinks=True):
                        continue

                    skill_md_path = os.path.join(entry.path, SKILL_METADATA_FILE)
                    if not os.path.isfile(skill_md_path):
                        continue

                    abs_path = os.path.abspath(entry.path)

                    # Check cache: skip if mtime hasn't changed
                    if not force_refresh and abs_path in self._cache:
                        try:
                            current_mtime = os.path.getmtime(skill_md_path)
                            if current_mtime == self._cache[abs_path]:
                                results.append(abs_path)
                                continue
                        except OSError:
                            pass

                    # Update cache with current mtime
                    try:
                        self._cache[abs_path] = os.path.getmtime(skill_md_path)
                    except OSError:
                        self._cache[abs_path] = 0.0

                    results.append(abs_path)
                    logger.debug(f"Found skill at: {abs_path}")

        except PermissionError:
            logger.warning(f"Permission denied scanning: {search_path}")
        except OSError as e:
            logger.warning(f"Error scanning directory {search_path}: {e}")

        return results

    def clear_cache(self) -> None:
        """Clear the scan cache, forcing a full rescan on next call."""
        self._cache.clear()
        self._skill_dirs.clear()
        logger.debug("Cleared skill scanner cache")

    @property
    def discovered_skills(self) -> List[str]:
        """Return the list of discovered skill directories from the last scan."""
        return list(self._skill_dirs)


def scan_skill_paths(
    extra_paths: Optional[List[str]] = None,
    dcc_name: Optional[str] = None,
) -> List[str]:
    """Convenience function to scan for skills using a fresh scanner.

    Args:
        extra_paths: Additional paths to scan.
        dcc_name: Optional DCC name for DCC-specific skills directory.

    Returns:
        List of discovered skill directory paths.

    """
    scanner = SkillScanner()
    return scanner.scan(extra_paths=extra_paths, dcc_name=dcc_name)
