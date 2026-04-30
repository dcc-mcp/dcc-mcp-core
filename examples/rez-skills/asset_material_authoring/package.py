# ruff: noqa: F821

name = "asset_material_authoring"
version = "1.0.0"
requires = ["dcc_mcp_core"]


def commands():
    env.DCC_MCP_CONTEXT_BUNDLE = "asset.materials.fabric-library.authoring"
    env.DCC_MCP_PRODUCTION_DOMAIN = "asset"
    env.DCC_MCP_CONTEXT_KIND = "asset"
    env.DCC_MCP_PROJECT = "shared-assets"
    env.DCC_MCP_ASSET = "fabric-library"
    env.DCC_MCP_ASSET_TYPE = "material"
    env.DCC_MCP_TASK = "material-authoring"
    env.DCC_MCP_TOOLSET_PROFILE = "asset-material-authoring"
    env.DCC_MCP_PACKAGE_PROVENANCE.append("asset_material_authoring-1.0.0")
    env.DCC_MCP_SKILL_PATHS.append("{root}/skills")
