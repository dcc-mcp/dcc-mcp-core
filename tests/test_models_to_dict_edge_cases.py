"""Tests for the to_dict method edge cases in ActionResultModel."""

# Import built-in modules
from unittest.mock import patch

# Import third-party modules
from pydantic import BaseModel

# Import local modules
from dcc_mcp_core.models import ActionResultModel


class TestActionResultModelToDictEdgeCases:
    """Tests for the to_dict method edge cases in ActionResultModel."""

    def test_to_dict_pydantic_v1_fallback(self):
        """Test the Pydantic v1 fallback in to_dict method."""
        # Create a model instance
        model = ActionResultModel(message="Test message")

        # Create a patched version of hasattr that returns False for 'model_dump'
        original_hasattr = hasattr

        def patched_hasattr(obj, name):
            if name == "model_dump":
                return False
            return original_hasattr(obj, name)

        # Create a mock dict method that returns a known dictionary
        expected_dict = {"success": True, "message": "Test message", "prompt": None, "error": None, "context": {}}

        # Apply patches
        with patch("dcc_mcp_core.models.hasattr", patched_hasattr):
            with patch.object(ActionResultModel, "dict", return_value=expected_dict):
                # Call to_dict
                result = model.to_dict()

                # Verify result
                assert result == expected_dict

    def test_to_dict_exception_handling(self):
        """Test the exception handling in to_dict method."""
        # Create a model instance
        model = ActionResultModel(
            success=True, message="Test message", prompt="Test prompt", error=None, context={"key": "value"}
        )

        # Create a patch that raises an exception when hasattr is called
        def raise_exception(*args, **kwargs):
            raise Exception("Test exception")

        # Apply patch
        with patch("dcc_mcp_core.models.hasattr", side_effect=raise_exception):
            # Call to_dict
            result = model.to_dict()

            # Verify result is manually created dictionary
            assert result["success"] is True
            assert result["message"] == "Test message"
            assert result["prompt"] == "Test prompt"
            assert result["error"] is None
            assert result["context"] == {"key": "value"}

    def test_to_dict_model_dump_exception(self):
        """Test handling of exception in model_dump."""
        # Create a model instance
        model = ActionResultModel(message="Test message")

        # Create patches that simulate model_dump and dict raising exceptions
        original_hasattr = hasattr

        def patched_hasattr(obj, name):
            if name == "model_dump":
                return True
            return original_hasattr(obj, name)

        def raise_exception(*args, **kwargs):
            raise Exception("Test exception")

        # Apply patches
        with patch("dcc_mcp_core.models.hasattr", patched_hasattr):
            # We need to patch the actual model_dump method on the class
            # Since we can't directly patch model.model_dump (it doesn't exist yet)
            with patch.object(BaseModel, "model_dump", side_effect=raise_exception):
                # Also patch dict method to raise exception
                with patch.object(BaseModel, "dict", side_effect=raise_exception):
                    # Call to_dict
                    result = model.to_dict()

                    # Verify fallback to manual dictionary creation
                    assert result["success"] is True
                    assert result["message"] == "Test message"
                    assert result["prompt"] is None
                    assert result["error"] is None
                    assert result["context"] == {}
