# ruff: noqa: F821

name = "commercial_2d_layers"
version = "1.0.0"
requires = ["dcc_mcp_core", "dcc_mcp_photoshop"]


def commands():
    env.DCC_MCP_CONTEXT_BUNDLE = "ad-spot.deliverable.social-16x9.motion"
    env.DCC_MCP_PRODUCTION_DOMAIN = "advertising"
    env.DCC_MCP_CONTEXT_KIND = "deliverable"
    env.DCC_MCP_PROJECT = "ad-spot"
    env.DCC_MCP_TASK = "motion-2d"
    env.DCC_MCP_TOOLSET_PROFILE = "commercial-2d-motion"
    env.DCC_MCP_PACKAGE_PROVENANCE.append("commercial_2d_layers-1.0.0")
    env.DCC_MCP_PHOTOSHOP_SKILL_PATHS.append("{root}/skills")
