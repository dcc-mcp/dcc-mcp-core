"""ScriptAction factory for dynamically generating Action subclasses.

This module provides the create_script_action factory function that takes a
script file path and skill metadata, and returns an Action subclass that
executes the script via subprocess or DCC adapter.
"""

# Import built-in modules
import logging
import os
import subprocess
import sys
from typing import Any
from typing import ClassVar
from typing import Dict
from typing import List
from typing import Optional
from typing import Type

# Import third-party modules
from pydantic import Field

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.constants import DEFAULT_DCC
from dcc_mcp_core.constants import SUPPORTED_SCRIPT_EXTENSIONS
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.models import SkillMetadata
from dcc_mcp_core.utils.result_factory import error_result
from dcc_mcp_core.utils.result_factory import success_result

logger = logging.getLogger(__name__)


def _make_action_name(skill_name: str, script_path: str) -> str:
    """Generate a unique action name from skill name and script filename.

    Args:
        skill_name: Name of the parent skill.
        script_path: Path to the script file.

    Returns:
        Action name in the format "skill_name__script_stem".

    """
    stem = os.path.splitext(os.path.basename(script_path))[0]
    # Normalize: replace hyphens with underscores, lowercase
    safe_skill = skill_name.replace("-", "_").replace(" ", "_").lower()
    safe_stem = stem.replace("-", "_").replace(" ", "_").lower()
    return f"{safe_skill}__{safe_stem}"


def _get_script_type(script_path: str) -> str:
    """Determine the script type from file extension.

    Args:
        script_path: Path to the script file.

    Returns:
        Script type string (e.g., "python", "mel", "shell").

    """
    ext = os.path.splitext(script_path)[1].lower()
    return SUPPORTED_SCRIPT_EXTENSIONS.get(ext, "unknown")


def create_script_action(
    skill_name: str,
    script_path: str,
    skill_metadata: SkillMetadata,
    dcc_name: str = DEFAULT_DCC,
) -> Type[Action]:
    """Dynamically create an Action subclass for a script file.

    The generated Action:
    - name = "{skill_name}__{script_stem}"
    - description from skill metadata or script filename
    - _execute() runs the script via subprocess or DCC adapter
    - InputModel has 'args' field for script arguments

    Args:
        skill_name: Name of the parent skill.
        script_path: Absolute path to the script file.
        skill_metadata: Parsed SkillMetadata from SKILL.md.
        dcc_name: Target DCC name for the action.

    Returns:
        A new Action subclass class object.

    """
    action_name = _make_action_name(skill_name, script_path)
    script_type = _get_script_type(script_path)
    script_basename = os.path.basename(script_path)
    abs_script_path = os.path.abspath(script_path)

    # Build description
    description = (
        f"Execute {script_basename} from skill '{skill_name}'. "
        f"Type: {script_type}. {skill_metadata.description}"
    ).strip()

    class ScriptInputModel(Action.InputModel):
        """Input parameters for script execution."""

        args: List[str] = Field(default_factory=list, description="Command-line arguments to pass to the script")
        working_dir: Optional[str] = Field(None, description="Working directory for script execution")
        timeout: int = Field(default=300, description="Execution timeout in seconds", ge=1, le=3600)
        env_vars: Dict[str, str] = Field(default_factory=dict, description="Additional environment variables")

    class ScriptOutputModel(Action.OutputModel):
        """Output data from script execution."""

        stdout: str = Field(default="", description="Standard output from the script")
        stderr: str = Field(default="", description="Standard error from the script")
        return_code: int = Field(default=0, description="Script exit code")
        script_path: str = Field(default="", description="Path to the executed script")
        script_type: str = Field(default="", description="Type of script executed")

    # Create the Action subclass dynamically
    action_attrs: Dict[str, Any] = {
        # Class metadata
        "name": action_name,
        "description": description,
        "tags": list(skill_metadata.tags) + [script_type, "skill", skill_name],
        "category": f"skill:{skill_name}",
        "dcc": dcc_name,
        "abstract": False,

        # Nested models
        "InputModel": ScriptInputModel,
        "OutputModel": ScriptOutputModel,

        # Store references for _execute
        "_script_path": abs_script_path,
        "_script_type": script_type,
        "_skill_metadata": skill_metadata,

        # Implement _execute
        "_execute": _make_execute_method(abs_script_path, script_type),
    }

    # Create the class
    action_class = type(f"ScriptAction_{action_name}", (Action,), action_attrs)

    # Ensure ClassVar annotations are set properly
    action_class.name = action_name  # type: ignore[attr-defined]
    action_class.description = description  # type: ignore[attr-defined]

    return action_class  # type: ignore[return-value]


def _make_execute_method(script_path: str, script_type: str):
    """Create the _execute method for a ScriptAction.

    Args:
        script_path: Absolute path to the script file.
        script_type: Type of the script (python, shell, mel, etc.).

    Returns:
        A function suitable as an Action._execute method.

    """

    def _execute(self: Action) -> None:
        """Execute the script and set output."""
        input_data = self.input
        args = list(input_data.args) if input_data.args else []
        working_dir = input_data.working_dir
        timeout = input_data.timeout
        extra_env = dict(input_data.env_vars) if input_data.env_vars else {}

        # Build command based on script type
        if script_type == "python":
            cmd = [sys.executable, script_path] + args
        elif script_type in ("shell", "bash"):
            cmd = ["bash", script_path] + args
        elif script_type == "batch":
            cmd = ["cmd", "/c", script_path] + args
        elif script_type == "powershell":
            cmd = ["powershell", "-ExecutionPolicy", "Bypass", "-File", script_path] + args
        elif script_type in ("mel", "maxscript"):
            # MEL/MaxScript need DCC adapter from context
            dcc_adapter = self.context.get("dcc_adapter")
            if dcc_adapter and hasattr(dcc_adapter, "execute"):
                try:
                    with open(script_path, "r", encoding="utf-8") as f:
                        script_content = f.read()
                    result = dcc_adapter.execute(script_content, script_type=script_type)
                    self.output = self.OutputModel(
                        stdout=str(result.get("output", "")),
                        stderr=str(result.get("error", "")),
                        return_code=0 if result.get("success", True) else 1,
                        script_path=script_path,
                        script_type=script_type,
                    )
                    return
                except Exception as e:
                    self.output = self.OutputModel(
                        stdout="",
                        stderr=str(e),
                        return_code=1,
                        script_path=script_path,
                        script_type=script_type,
                    )
                    return
            else:
                self.output = self.OutputModel(
                    stdout="",
                    stderr=f"No DCC adapter available for {script_type} scripts. "
                    f"Provide 'dcc_adapter' in context.",
                    return_code=1,
                    script_path=script_path,
                    script_type=script_type,
                )
                return
        elif script_type == "javascript":
            cmd = ["node", script_path] + args
        else:
            cmd = [script_path] + args

        # Prepare environment
        env = os.environ.copy()
        env.update(extra_env)

        # Execute via subprocess
        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=timeout,
                cwd=working_dir,
                env=env,
            )

            self.output = self.OutputModel(
                stdout=result.stdout,
                stderr=result.stderr,
                return_code=result.returncode,
                script_path=script_path,
                script_type=script_type,
                prompt=f"Script {os.path.basename(script_path)} executed with exit code {result.returncode}.",
            )
        except subprocess.TimeoutExpired:
            self.output = self.OutputModel(
                stdout="",
                stderr=f"Script execution timed out after {timeout} seconds",
                return_code=-1,
                script_path=script_path,
                script_type=script_type,
            )
        except FileNotFoundError as e:
            self.output = self.OutputModel(
                stdout="",
                stderr=f"Command not found: {e}",
                return_code=-1,
                script_path=script_path,
                script_type=script_type,
            )
        except Exception as e:
            self.output = self.OutputModel(
                stdout="",
                stderr=str(e),
                return_code=-1,
                script_path=script_path,
                script_type=script_type,
            )

    return _execute
