# ruff: noqa: F821

name = "film_animation_blocking"
version = "1.0.0"
requires = ["dcc_mcp_core", "dcc_mcp_maya"]


def commands():
    env.DCC_MCP_CONTEXT_BUNDLE = "show-a.seq010.shot020.animation"
    env.DCC_MCP_PRODUCTION_DOMAIN = "film"
    env.DCC_MCP_CONTEXT_KIND = "shot"
    env.DCC_MCP_PROJECT = "show-a"
    env.DCC_MCP_SEQUENCE = "seq010"
    env.DCC_MCP_SHOT = "shot020"
    env.DCC_MCP_TASK = "animation"
    env.DCC_MCP_TOOLSET_PROFILE = "film-shot-animation"
    env.DCC_MCP_PACKAGE_PROVENANCE.append("film_animation_blocking-1.0.0")
    env.DCC_MCP_MAYA_SKILL_PATHS.append("{root}/skills")
