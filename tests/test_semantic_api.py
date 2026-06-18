import pytest

def test_semantic_public_api():
    """Ensure the expected public API is available for dcc_mcp_core_semantic."""
    try:
        import dcc_mcp_core_semantic
    except ImportError:
        pytest.skip("dcc-mcp-core-semantic not installed")

    # NativeEmbedder should be available at the top level
    assert hasattr(dcc_mcp_core_semantic, "NativeEmbedder")
    
    # OnnxEmbedder should be available as a compatibility alias
    assert hasattr(dcc_mcp_core_semantic, "OnnxEmbedder")
    
    # They should be the same class
    assert dcc_mcp_core_semantic.NativeEmbedder is dcc_mcp_core_semantic.OnnxEmbedder
    
    # Try instantiating it if possible (may require downloading model, so we skip the actual init or catch exceptions)
    # The issue just asks for an import path smoke test.
