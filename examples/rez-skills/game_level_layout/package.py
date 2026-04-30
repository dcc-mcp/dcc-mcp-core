# ruff: noqa: F821

name = "game_level_layout"
version = "1.0.0"
requires = ["dcc_mcp_core", "dcc_mcp_unreal"]


def commands():
    env.DCC_MCP_CONTEXT_BUNDLE = "game-demo.level.city-block.layout"
    env.DCC_MCP_PRODUCTION_DOMAIN = "game"
    env.DCC_MCP_CONTEXT_KIND = "level"
    env.DCC_MCP_PROJECT = "game-demo"
    env.DCC_MCP_ASSET = "city-block"
    env.DCC_MCP_TASK = "level-layout"
    env.DCC_MCP_TOOLSET_PROFILE = "game-level-layout"
    env.DCC_MCP_PACKAGE_PROVENANCE.append("game_level_layout-1.0.0")
    env.DCC_MCP_UNREAL_SKILL_PATHS.append("{root}/skills")
