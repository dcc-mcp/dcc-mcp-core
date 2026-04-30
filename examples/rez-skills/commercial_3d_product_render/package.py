# ruff: noqa: F821

name = "commercial_3d_product_render"
version = "1.0.0"
requires = ["dcc_mcp_core", "dcc_mcp_blender"]


def commands():
    env.DCC_MCP_CONTEXT_BUNDLE = "product.launch-pack.hero-render.lookdev"
    env.DCC_MCP_PRODUCTION_DOMAIN = "advertising"
    env.DCC_MCP_CONTEXT_KIND = "product"
    env.DCC_MCP_PROJECT = "launch-pack"
    env.DCC_MCP_ASSET = "hero-product"
    env.DCC_MCP_TASK = "product-render"
    env.DCC_MCP_TOOLSET_PROFILE = "commercial-3d-product-render"
    env.DCC_MCP_PACKAGE_PROVENANCE.append("commercial_3d_product_render-1.0.0")
    env.DCC_MCP_BLENDER_SKILL_PATHS.append("{root}/skills")
