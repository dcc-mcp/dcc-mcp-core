"""Example demonstrating the use of ActionManager in DCC-MCP-Core.

This example shows how to discover, load, and manage actions using the
restructured ActionManager class.
"""

import logging
import sys
from pathlib import Path

# Configure logging
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)

# Add the parent directory to the Python path
sys.path.append('..')

# Import DCC-MCP-Core modules
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.filesystem import get_action_paths


def list_registered_dccs():
    """List all registered DCCs in the system."""
    from dcc_mcp_core.filesystem import get_all_registered_dccs
    
    dccs = get_all_registered_dccs()
    logger.info(f"Registered DCCs: {dccs}")
    return dccs


def explore_action_paths():
    """Explore action paths for different DCCs."""
    # Get action paths for all DCCs
    all_paths = get_action_paths()
    logger.info("Action paths for all DCCs:")
    for dcc, paths in all_paths.items():
        logger.info(f"  {dcc}: {paths}")
    
    # Get action paths for a specific DCC
    maya_paths = get_action_paths('maya')
    logger.info(f"\nMaya action paths: {maya_paths}")


def discover_and_load_actions(dcc_name):
    """Discover and load actions for a specific DCC.
    
    Args:
        dcc_name: Name of the DCC to discover actions for
    """
    # Create an action manager
    manager = ActionManager(dcc_name)
    
    # Discover actions
    action_paths = manager.discover_actions()
    logger.info(f"Discovered {len(action_paths)} actions for {dcc_name}")
    
    # Load actions
    loaded_actions = manager.load_actions(action_paths)
    logger.info(f"Loaded {len(loaded_actions)} actions for {dcc_name}")
    
    # Get action info
    actions_info = manager.get_actions_info()
    
    # Display action information
    logger.info(f"\nActions for {dcc_name}:")
    for name, info in actions_info.items():
        logger.info(f"  Action: {name}")
        logger.info(f"    Version: {info.get('version', 'N/A')}")
        logger.info(f"    Description: {info.get('description', 'N/A')}")
        logger.info(f"    Functions:")
        for func_name, func_info in info.get('functions', {}).items():
            logger.info(f"      - {func_name}: {func_info.get('description', 'N/A')}")
        logger.info("")
    
    return manager


def main():
    """Run the example."""
    logger.info("=== DCC-MCP-Core Action Manager Example ===")
    
    # List registered DCCs
    logger.info("\n1. Registered DCCs:")
    dccs = list_registered_dccs()
    
    # Explore action paths
    logger.info("\n2. Action Paths:")
    explore_action_paths()
    
    # Discover and load actions
    if 'maya' in dccs:
        logger.info("\n3. Discover and Load Actions for Maya:")
        maya_manager = discover_and_load_actions('maya')
    else:
        logger.info("\n3. Maya not registered, using a mock DCC name for demonstration:")
        mock_manager = discover_and_load_actions('mock_dcc')
    
    logger.info("\n=== Example Complete ===")


if __name__ == "__main__":
    main()
