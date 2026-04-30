# ruff: noqa: F821

name = "film_fx_cache_review"
version = "1.0.0"
requires = ["dcc_mcp_core", "dcc_mcp_houdini"]


def commands():
    env.DCC_MCP_CONTEXT_BUNDLE = "show-a.seq010.shot020.fx"
    env.DCC_MCP_PRODUCTION_DOMAIN = "film"
    env.DCC_MCP_CONTEXT_KIND = "shot"
    env.DCC_MCP_PROJECT = "show-a"
    env.DCC_MCP_SEQUENCE = "seq010"
    env.DCC_MCP_SHOT = "shot020"
    env.DCC_MCP_TASK = "fx"
    env.DCC_MCP_TOOLSET_PROFILE = "film-shot-fx"
    env.DCC_MCP_PACKAGE_PROVENANCE.append("film_fx_cache_review-1.0.0")
    env.DCC_MCP_HOUDINI_SKILL_PATHS.append("{root}/skills")
