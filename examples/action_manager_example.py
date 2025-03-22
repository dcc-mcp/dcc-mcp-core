"""Example demonstrating the use of ActionManager in DCC-MCP-Core.

This example shows how to discover, load, and manage actions using the
restructured ActionManager class.
"""

# Import built-in modules
from pathlib import Path

# Import local modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.logg_config import set_log_level
from dcc_mcp_core.logg_config import setup_logging
from dcc_mcp_core.utils.filesystem import clear_dcc_actions_paths
from dcc_mcp_core.utils.filesystem import get_action_paths
from dcc_mcp_core.utils.filesystem import register_dcc_actions_path
from dcc_mcp_core.utils.filesystem import save_actions_paths_config

set_log_level("DEBUG")

logger = setup_logging("action_manager_example")
logger.info("Starting action_manager_example")


def explore_action_paths():
    """Explore action paths for all DCCs."""
    # Get action paths for all DCCs
    action_paths = get_action_paths()

    # Display action paths
    logger.info("Action paths for all DCCs:")
    for dcc_name, paths in action_paths.items():
        logger.info(f"  {dcc_name}: {paths}")

    return action_paths


def register_example_actions(dcc_name):
    """Register example actions for a specific DCC.

    Args:
        dcc_name: Name of the DCC to register actions for

    Returns:
        Path to the registered actions directory

    """
    # Get the current directory
    current_dir = Path(__file__).parent.resolve()
    logger.debug(f"Current directory: {current_dir}")

    # Register Maya actions directory
    maya_actions_dir = current_dir / "maya" / "actions"

    # Check if directory exists
    if not maya_actions_dir.exists():
        logger.warning(f"Maya actions directory does not exist: {maya_actions_dir}")
        logger.info("Creating Maya actions directory")
        maya_actions_dir.mkdir(parents=True, exist_ok=True)

    # 检查目录中是否有 Python 文件
    python_files = list(maya_actions_dir.glob("*.py"))
    logger.debug(f"Found {len(python_files)} Python files in {maya_actions_dir}")
    for py_file in python_files:
        logger.debug(f"  - {py_file.name}")

    # Register Maya actions directory
    logger.info(f"Registering Maya actions directory: {maya_actions_dir}")
    register_dcc_actions_path("maya", str(maya_actions_dir))

    # Register Python actions directory
    python_actions_dir = current_dir / "python"
    if python_actions_dir.exists():
        python_files = list(python_actions_dir.glob("*.py"))
        logger.debug(f"Found {len(python_files)} Python files in {python_actions_dir}")
        for py_file in python_files:
            logger.debug(f"  - {py_file.name}")

        logger.info(f"Registering Python actions directory: {python_actions_dir}")
        register_dcc_actions_path("python", str(python_actions_dir))
    else:
        logger.warning(f"Python actions directory does not exist: {python_actions_dir}")

    # Save configuration
    save_actions_paths_config()

    # Return Maya actions directory (for compatibility)
    return str(maya_actions_dir)


def discover_and_load_actions(dcc_name):
    """Discover and load actions for a specific DCC.

    This function performs the following steps:
    1. Register example actions for the specified DCC
    2. Create an ActionManager instance
    3. Discover available actions (returns ActionResultModel)
    4. Load discovered actions (returns ActionResultModel)
    5. Get detailed information about loaded actions (returns ActionResultModel)
    6. Display information about loaded actions

    Note: All ActionManager methods decorated with @method_error_handler return ActionResultModel.
    The actual return value (e.g., ActionModel, ActionsInfoModel) is stored in the
    context['result'] attribute of the ActionResultModel.

    Args:
        dcc_name: Name of the DCC to discover actions for (e.g., 'maya', 'houdini')

    Returns:
        ActionManager instance with loaded actions

    """
    # Initialize statistics
    stats = {
        "discovered_paths": 0,
        "loaded_actions": 0,
        "failed_actions": 0,
        "successful_actions": 0
    }

    # Register example actions first
    register_example_actions(dcc_name)
    logger.info(f"Registered example actions for {dcc_name}")

    # Create an action manager
    manager = ActionManager(dcc_name)
    logger.info(f"Created ActionManager for {dcc_name}")

    # Step 1: Discover actions - returns an ActionResultModel
    logger.info(f"\n=== Discovering actions for {dcc_name} ===")
    result_model = manager.discover_actions()
    logger.info(f"Discovery result: success={result_model.success}, message='{result_model.message}'")

    # Handle discovery result
    if not result_model.success:
        logger.error(f"Failed to discover actions: {result_model.error}")
        return manager

    # Get discovered paths
    if 'paths' not in result_model.context:
        logger.error("No 'paths' found in result_model.context")
        return manager

    action_paths = result_model.context['paths']
    stats["discovered_paths"] = len(action_paths)
    logger.info(f"Discovered {len(action_paths)} action paths for {dcc_name}")

    # Step 2: Load actions - returns ActionResultModel
    logger.info(f"\n=== Loading actions for {dcc_name} ===")
    loaded_result = manager.load_actions(action_paths)
    logger.info(f"Loading result: success={loaded_result.success}, message='{loaded_result.message}'")

    # Handle loading result
    if not loaded_result.success:
        logger.error(f"Failed to load actions: {loaded_result.error}")
        return manager

    # Get loaded actions information
    if 'result' not in loaded_result.context:
        logger.error("No 'result' found in loaded_result.context")
        return manager

    actions_info = loaded_result.context['result']

    # Ensure actions_info has actions attribute
    if not hasattr(actions_info, 'actions'):
        logger.error("actions_info does not have 'actions' attribute")
        return manager

    actions_dict = actions_info.actions
    stats["loaded_actions"] = len(actions_dict)
    logger.info(f"Loaded {len(actions_dict)} actions for {dcc_name}")

    # Step 3: Get detailed information about loaded actions
    logger.info("\n=== Getting detailed information about loaded actions ===")

    for action_name in list(actions_dict.keys()):
        # get_action_info returns ActionResultModel
        action_info_result = manager.get_action_info(action_name)

        if not action_info_result.success:
            logger.warning(f"Failed to get info for action '{action_name}': {action_info_result.error}")
            stats["failed_actions"] += 1
            continue

        if 'result' not in action_info_result.context:
            logger.warning(f"No 'result' found in context for action '{action_name}'")
            stats["failed_actions"] += 1
            continue

        # Get ActionModel from context['result']
        action_model = action_info_result.context['result']
        stats["successful_actions"] += 1

        # Display action information
        logger.info(f"\nAction: {action_model.name} (v{action_model.version})")
        logger.info(f"Description: {action_model.description}")
        logger.info(f"Author: {action_model.author}")

        # Display function information
        if action_model.functions:
            logger.info("Functions:")
            for func_name, func_info in action_model.functions.items():
                logger.info(f"  - {func_name}: {func_info.description}")

    # Display statistics
    logger.info("\n=== Action Loading Statistics ===")
    logger.info(f"Discovered paths: {stats['discovered_paths']}")
    logger.info(f"Loaded actions: {stats['loaded_actions']}")
    logger.info(f"Successful actions: {stats['successful_actions']}")
    logger.info(f"Failed actions: {stats['failed_actions']}")

    return manager


def test_get_actions_info(manager):
    """Test getting information about all loaded actions.

    Args:
        manager: ActionManager instance with loaded actions

    """
    logger.info("\n=== Testing get_actions_info ===")

    # Get information about all loaded actions
    result = manager.get_actions_info()

    if not result.success:
        logger.error(f"Failed to get actions info: {result.error}")
        return

    if 'result' not in result.context:
        logger.error("No 'result' found in context")
        return

    actions_info = result.context['result']
    logger.info(f"Got information about {len(actions_info.actions)} actions")

    # Display information about each action
    for action_name, action_model in actions_info.actions.items():
        logger.info(f"\nAction: {action_model.name} (v{action_model.version})")
        logger.info(f"Description: {action_model.description}")
        logger.info(f"Author: {action_model.author}")

        # Display information about functions
        if action_model.functions:
            logger.info("Functions:")
            for func_name, func_info in action_model.functions.items():
                logger.info(f"  - {func_name}: {func_info.description}")


def test_get_action_info(manager, action_name):
    """Test getting information about a specific action.

    Args:
        manager: ActionManager instance with loaded actions
        action_name: Name of the action to get information for

    """
    logger.info(f"\n=== Testing get_action_info for '{action_name}' ===")

    # Get information about the action
    result = manager.get_action_info(action_name)

    if not result.success:
        logger.error(f"Failed to get action info: {result.error}")
        return

    if 'result' not in result.context:
        logger.error("No 'result' found in context")
        return

    action_model = result.context['result']
    logger.info(f"Action: {action_model.name} (v{action_model.version})")
    logger.info(f"Description: {action_model.description}")
    logger.info(f"Author: {action_model.author}")

    # Display information about functions
    if action_model.functions:
        logger.info("Functions:")
        for func_name, func_info in action_model.functions.items():
            logger.info(f"  - {func_name}: {func_info.description}")

            # Display information about parameters
            if func_info.parameters:
                logger.info("    Parameters:")
                for param_info in func_info.parameters:
                    logger.info(f"      - {param_info.name}: {param_info.description} (type: {param_info.type})")


def test_create_sphere(manager, radius=1.5, position=None):
    """Test calling the create_sphere function from Random Spheres Generator action.

    Args:
        manager: ActionManager instance with loaded actions
        radius: Radius of the sphere to create
        position: Position of the sphere [x, y, z], defaults to None which will use [0, 0, 0]

    """
    # Check if the manager has loaded actions
    actions_info = manager.get_actions_info()

    # Convert ActionsInfoModel to dictionary if needed
    if hasattr(actions_info, 'actions'):
        actions_dict = actions_info.actions
    else:
        actions_dict = actions_info

    if not actions_dict:
        logger.error("No actions loaded in the manager")
        return

    # Check if the Random Spheres Generator action is loaded
    if 'Random Spheres Generator' not in actions_dict:
        logger.error("Random Spheres Generator action not loaded")
        logger.info(f"Available actions: {list(actions_dict.keys() if hasattr(actions_dict, 'keys') else [])}")
        return

    # Create a mock context with maya_client
    # In a real scenario, this would be provided by the MCP server
    # For testing purposes, we'll create a minimal context
    context = {
        "maya_client": {
            "cmds": None  # This would normally be the Maya commands interface
        }
    }

    # Call the function using call_action_function method
    result = manager.call_action_function(
        'Random Spheres Generator',
        'create_sphere',
        context=context,
        radius=radius,
        position=position
    )
    logger.info(f"create_sphere result: {result}")
    if hasattr(result, 'success'):
        logger.info(f"Success: {result.success}")
        logger.info(f"Message: {result.message}")
        if hasattr(result, 'error') and result.error:
            logger.error(f"Error: {result.error}")
    else:
        logger.info(f"Raw result: {result}")
    if result.success:
        logger.info("create_sphere function call was successful")
    else:
        logger.error("create_sphere function call failed")


def main():
    """Run the example."""
    try:
        print("\n" + "=" * 80)
        logger.info("=== DCC-MCP-Core Action Manager Example ===")
        print("=" * 80 + "\n")

        # Clear any existing action paths
        clear_dcc_actions_paths()
        logger.info("Cleared existing action paths")

        # Explore action paths
        print("\n" + "-" * 40)
        logger.info("1. Exploring action paths:")
        paths = explore_action_paths()
        logger.info(f"Found action paths: {paths}")

        # Use 'maya' as the DCC name for this example
        dcc_name = "maya"
        print("\n" + "-" * 40)
        logger.info(f"2. Using DCC: {dcc_name}")

        # Register example actions
        print("\n" + "-" * 40)
        logger.info("3. Registering example actions:")
        example_dir = register_example_actions(dcc_name)
        logger.info(f"Example actions directory: {example_dir}")

        # Verify the directory exists and contains Python files
        example_dir_path = Path(example_dir)
        if not example_dir_path.exists():
            logger.error(f"Example directory does not exist: {example_dir}")
            return

        python_files = list(example_dir_path.glob("*.py"))
        if not python_files:
            logger.warning(f"No Python files found in {example_dir}")

        # Create an ActionManager instance
        print("\n" + "-" * 40)
        logger.info("4. Creating ActionManager instance:")
        manager = ActionManager(dcc_name)
        logger.info(f"Created ActionManager for {dcc_name}")

        # Discover and load actions
        print("\n" + "-" * 40)
        logger.info("5. Discovering and loading actions:")
        available_actions = discover_and_load_actions(dcc_name)

        # Test get_actions_info method
        print("\n" + "-" * 40)
        logger.info("6. Testing get_actions_info method:")
        test_get_actions_info(manager)

        # Test get_action_info method if actions are available
        if available_actions:
            action_name = available_actions[0]
            logger.info(f"Testing with action: {action_name}")
            test_get_action_info(manager, action_name)
        else:
            logger.warning("No actions available for testing get_action_info")

        # Test calling a function if actions are available
        if available_actions:
            print("\n" + "-" * 40)
            logger.info("7. Testing action function call:")
            test_create_sphere(manager)

        # Create Python ActionManager instance and test Python actions
        print("\n" + "-" * 40)
        logger.info("8. Testing Python actions:")
        python_manager = ActionManager("python")
        logger.info("Created ActionManager for python")

        # Discover and load Python actions
        python_actions = discover_and_load_actions("python")

        # Test get_actions_info method
        logger.info("Testing get_actions_info method for Python:")
        test_get_actions_info(python_manager)

        # Test get_action_info method
        if python_actions:
            action_name = python_actions[0]
            logger.info(f"Testing with Python action: {action_name}")
            test_get_action_info(python_manager, action_name)

            # Test calling Python action function
            logger.info("Testing Python action function call:")
            try:
                context = {}
                result = python_manager.call_action_function(action_name, 'print_info', context=context)
                logger.info(f"print_info result: {result}")
                if hasattr(result, 'success'):
                    logger.info(f"Success: {result.success}")
                    logger.info(f"Message: {result.message}")
                    if hasattr(result, 'error') and result.error:
                        logger.error(f"Error: {result.error}")
                else:
                    logger.info(f"Raw result: {result}")
            except Exception as e:
                logger.error(f"Error calling print_info: {e}")
        else:
            logger.warning("No Python actions available for testing")

        print("\n" + "-" * 40)
        logger.info("=== Example Complete ===")

    except Exception as e:
        logger.error(f"Error in example: {e}")
        # Import built-in modules
        import traceback
        traceback.print_exc()


def create_sample_action(directory):
    """Create a sample action file for testing.

    Args:
        directory: Directory to create the sample action in

    """
    sample_file = directory / "sample_action.py"

    # Simple sample action code
    sample_code = '''\
"""Sample action for testing."""

# Action metadata
__action_name__ = "sample_action"
__action_version__ = "1.0.0"
__action_description__ = "A sample action for testing purposes"
__action_author__ = "DCC-MCP-Core"
__action_tags__ = ["sample", "test"]


def hello_world(name="World"):
    """Say hello to someone.

    Args:
        name: Name to greet

    Returns:
        Greeting message
    """
    message = f"Hello, {name}!"
    print(message)
    return {
        "success": True,
        "message": message,
        "context": {"name": name}
    }
'''

    # Write sample action file
    with open(sample_file, "w") as f:
        f.write(sample_code)

    logger.info(f"Created sample action file: {sample_file}")


if __name__ == "__main__":
    main()
