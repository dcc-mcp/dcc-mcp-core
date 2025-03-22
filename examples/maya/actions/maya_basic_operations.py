"""Maya basic operations plugin for DCC-MCP-Core.

This plugin demonstrates basic operations such as launching Maya,
writing to files, and basic file operations through RPyC remote execution.
"""

# Import built-in modules
import datetime
import os
import subprocess
import sys
from typing import Any
from typing import Dict
from typing import Optional

# Import local modules
from dcc_mcp_core.actions.manager import ActionResultModel
from dcc_mcp_core.utils.decorators import error_handler

# -------------------------------------------------------------------
# Plugin Metadata - Just fill in these basic information
# -------------------------------------------------------------------
__action_name__ = "Maya Basic Operations"
__action_version__ = "1.0.0"
__action_description__ = "Basic Maya operations like launching software and file operations"
__action_author__ = "DCC-MCP-Core Team"
__action_requires__ = ["maya"]  # Specify the DCC environment this plugin depends on

# -------------------------------------------------------------------
# Plugin Function Implementation
# -------------------------------------------------------------------

@error_handler
def launch_maya(context: Dict[str, Any], version: str = "", project_path: Optional[str] = None) -> Dict[str, Any]:
    """Launch Maya software.

    Args:
        context: Context object provided by the MCP server
        version: Maya version to launch (e.g., "2023", "2024"). If empty, launches default version.
        project_path: Path to set as the Maya project. If None, uses Maya's default project.

    Returns:
        Dictionary containing launch result

    """
    try:
        # Determine the Maya executable path based on platform
        maya_exe = "maya.exe" if sys.platform == "win32" else "maya"

        # Add version if specified
        if version:
            if sys.platform == "win32":
                maya_exe = f"maya{version}.exe"
            else:
                maya_exe = f"maya{version}"

        # Build command arguments
        cmd_args = [maya_exe]

        # Add project path if specified
        if project_path:
            cmd_args.extend(["-proj", project_path])

        # Launch Maya as a separate process
        process = subprocess.Popen(
            cmd_args,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            shell=True
        )

        # Wait a bit to check if process started successfully
        try:
            return_code = process.wait(timeout=1)
            if return_code != 0:
                stderr = process.stderr.read().decode("utf-8")
                return ActionResultModel(
                    success=False,
                    message=f"Maya failed to start with return code {return_code}",
                    error=stderr
                )
        except subprocess.TimeoutExpired:
            # Process is still running, which is good
            pass

        return ActionResultModel(
            success=True,
            message=f"Maya{' ' + version if version else ''} launched successfully",
            prompt="Maya is now running. You can perform operations in the Maya environment.",
            context={
                "process_id": process.pid,
                "version": version,
                "project_path": project_path
            }
        )
    except Exception as e:
        return ActionResultModel(
            success=False,
            message="Failed to launch Maya",
            error=str(e)
        )


@error_handler
def write_to_maya_script(context: Dict[str, Any], script_content: str,
                        file_name: Optional[str] = None,
                        script_type: str = "python") -> Dict[str, Any]:
    """Write content to a Maya script file.

    Args:
        context: Context object provided by the MCP server
        script_content: Content to write to the script file
        file_name: Name of the script file. If None, generates a name based on timestamp.
        script_type: Type of script ("python", "mel"). Determines file extension.

    Returns:
        Dictionary containing write operation result

    """
    # Get Maya client from context
    maya_client = context.get("maya_client", None)
    if not maya_client:
        return ActionResultModel(
            success=False,
            message="Cannot write script file",
            error="Maya client not found in context"
        )

    try:
        # Determine script extension based on type
        extension = ".py" if script_type.lower() == "python" else ".mel"

        # Generate file name if not provided
        if not file_name:
            timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
            file_name = f"maya_script_{timestamp}{extension}"
        elif not file_name.endswith(extension):
            file_name += extension

        # Get Maya script path
        # In a real scenario, we would use maya_client to get the script path
        # For this example, we'll use a default location
        if hasattr(maya_client, "cmds") and maya_client.cmds:
            cmds = maya_client.cmds
            script_path = cmds.internalVar(userScriptDir=True)
        else:
            # Fallback to a default path if cmds not available
            if sys.platform == "win32":
                script_path = os.path.expanduser("~/Documents/maya/scripts/")
            else:
                script_path = os.path.expanduser("~/maya/scripts/")

        # Ensure directory exists
        os.makedirs(script_path, exist_ok=True)

        # Full path to the script file
        full_path = os.path.join(script_path, file_name)

        # Write content to file
        with open(full_path, "w", encoding="utf-8") as f:
            f.write(script_content)

        return ActionResultModel(
            success=True,
            message=f"Script written to {full_path}",
            prompt="You can now source this script in Maya or run it with the execute_script function.",
            context={
                "file_path": full_path,
                "file_name": file_name,
                "script_type": script_type
            }
        )
    except Exception as e:
        return ActionResultModel(
            success=False,
            message="Failed to write script file",
            error=str(e)
        )


@error_handler
def execute_script(context: Dict[str, Any], script_path: str, script_type: Optional[str] = None) -> Dict[str, Any]:
    """Execute a script file in Maya.

    Args:
        context: Context object provided by the MCP server
        script_path: Path to the script file to execute
        script_type: Type of script ("python", "mel"). If None, determined from file extension.

    Returns:
        Dictionary containing execution result

    """
    # Get Maya client from context
    maya_client = context.get("maya_client", None)
    if not maya_client:
        return ActionResultModel(
            success=False,
            message="Cannot execute script",
            error="Maya client not found in context"
        )

    # Get Maya commands interface
    cmds = maya_client.cmds
    if not cmds:
        return ActionResultModel(
            success=False,
            message="Cannot execute script",
            error="Maya commands interface not found in client"
        )

    try:
        # Check if file exists
        if not os.path.isfile(script_path):
            return ActionResultModel(
                success=False,
                message=f"Script file not found: {script_path}",
                error="File does not exist"
            )

        # Determine script type from extension if not specified
        if not script_type:
            ext = os.path.splitext(script_path)[1].lower()
            script_type = "python" if ext == ".py" else "mel"

        # Execute the script based on its type
        result = None
        if script_type.lower() == "python":
            # For Python scripts
            with open(script_path, encoding="utf-8") as f:
                script_content = f.read()

            # Execute the Python script
            result = cmds.python(script_content)
        else:
            # For MEL scripts
            result = cmds.source(script_path)

        return ActionResultModel(
            success=True,
            message=f"Script executed successfully: {script_path}",
            prompt="Check the Maya console for any output from the script.",
            context={
                "script_path": script_path,
                "script_type": script_type,
                "execution_result": result
            }
        )
    except Exception as e:
        return ActionResultModel(
            success=False,
            message=f"Failed to execute script: {script_path}",
            error=str(e)
        )


@error_handler
def save_maya_file(context: Dict[str, Any], file_path: str, file_type: str = "mayaAscii") -> Dict[str, Any]:
    """Save the current Maya scene to a file.

    Args:
        context: Context object provided by the MCP server
        file_path: Path where to save the Maya file
        file_type: Type of Maya file ("mayaAscii" or "mayaBinary")

    Returns:
        Dictionary containing save operation result

    """
    # Get Maya client from context
    maya_client = context.get("maya_client", None)
    if not maya_client:
        return ActionResultModel(
            success=False,
            message="Cannot save Maya file",
            error="Maya client not found in context"
        )

    # Get Maya commands interface
    cmds = maya_client.cmds
    if not cmds:
        return ActionResultModel(
            success=False,
            message="Cannot save Maya file",
            error="Maya commands interface not found in client"
        )

    try:
        # Ensure directory exists
        directory = os.path.dirname(file_path)
        if directory and not os.path.exists(directory):
            os.makedirs(directory, exist_ok=True)

        # Determine file extension based on type if not already in path
        if file_type.lower() == "mayaascii" and not file_path.lower().endswith(".ma"):
            file_path += ".ma"
        elif file_type.lower() == "mayabinary" and not file_path.lower().endswith(".mb"):
            file_path += ".mb"

        # Save the file
        cmds.file(file_path, force=True, type=file_type, save=True)

        return ActionResultModel(
            success=True,
            message=f"Maya scene saved to {file_path}",
            prompt="The scene has been saved successfully.",
            context={
                "file_path": file_path,
                "file_type": file_type
            }
        )
    except Exception as e:
        return ActionResultModel(
            success=False,
            message="Failed to save Maya file",
            error=str(e)
        )
