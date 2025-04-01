"""Tests for the ActionResultModel in models.py."""

# Import built-in modules

# Import third-party modules
from pydantic import ValidationError
import pytest

# Import local modules
from dcc_mcp_core.models import ActionResultModel


class TestActionResultModel:
    """Tests for the ActionResultModel class."""

    def test_create_success_result(self):
        """Test creating a successful result model."""
        # Create a successful result
        result = ActionResultModel(
            success=True,
            message="Successfully created 3 spheres",
            prompt="You can now modify these spheres using the modify_spheres function",
            context={"created_objects": ["sphere1", "sphere2", "sphere3"], "total_count": 3},
        )

        # Verify the result
        assert result.success is True
        assert result.message == "Successfully created 3 spheres"
        assert result.prompt == "You can now modify these spheres using the modify_spheres function"
        assert result.error is None
        assert "created_objects" in result.context
        assert result.context["created_objects"] == ["sphere1", "sphere2", "sphere3"]
        assert result.context["total_count"] == 3

    def test_create_failure_result(self):
        """Test creating a failure result model."""
        # Create a failure result
        result = ActionResultModel(
            success=False,
            message="Failed to create spheres",
            prompt="Try reducing the number of objects or closing other scenes",
            error="Memory limit exceeded",
            context={
                "error_details": {
                    "code": "MEM_LIMIT",
                    "scene_stats": {"available_memory": "1.2MB", "required_memory": "5.0MB"},
                },
                "possible_solutions": [
                    "Reduce the number of objects",
                    "Close other scenes",
                    "Increase memory allocation",
                ],
            },
        )

        # Verify the result
        assert result.success is False
        assert result.message == "Failed to create spheres"
        assert result.prompt == "Try reducing the number of objects or closing other scenes"
        assert result.error == "Memory limit exceeded"
        assert "error_details" in result.context
        assert result.context["error_details"]["code"] == "MEM_LIMIT"
        assert "possible_solutions" in result.context
        assert len(result.context["possible_solutions"]) == 3

    def test_default_values(self):
        """Test default values for ActionResultModel."""
        # Create a minimal result
        result = ActionResultModel(message="Test message")

        # Verify default values
        assert result.success is True
        assert result.message == "Test message"
        assert result.prompt is None
        assert result.error is None
        assert result.context == {}

    def test_json_serialization(self):
        """Test JSON serialization of ActionResultModel."""
        # Create a result
        result = ActionResultModel(success=True, message="Test message", prompt="Test prompt", context={"key": "value"})

        # Convert to JSON and back
        json_data = result.model_dump_json()
        deserialized = ActionResultModel.model_validate_json(json_data)

        # Verify the deserialized result
        assert deserialized.success == result.success
        assert deserialized.message == result.message
        assert deserialized.prompt == result.prompt
        assert deserialized.error == result.error
        assert deserialized.context == result.context

    def test_required_fields(self):
        """Test required fields validation."""
        # Try to create a result without required fields
        with pytest.raises(ValidationError):
            ActionResultModel()

        # message is required
        with pytest.raises(ValidationError):
            ActionResultModel(success=True)

        # success is not required (has default)
        result = ActionResultModel(message="Test message")
        assert result.success is True

    def test_complex_context(self):
        """Test with a complex context structure."""
        # Create a complex context
        context = {
            "scene_info": {
                "objects": {"spheres": ["sphere1", "sphere2"], "cubes": ["cube1"]},
                "stats": {
                    "total_objects": 3,
                    "memory_usage": "1.5MB",
                    "performance": {"fps": 60, "render_time": "0.2s"},
                },
            },
            "user_settings": {"preferences": {"auto_save": True, "theme": "dark"}},
            "history": [
                {"action": "create", "time": "2023-01-01T12:00:00"},
                {"action": "modify", "time": "2023-01-01T12:05:00"},
            ],
        }

        # Create a result with complex context
        result = ActionResultModel(message="Operation completed", context=context)

        # Verify the context was stored correctly
        assert result.context["scene_info"]["objects"]["spheres"] == ["sphere1", "sphere2"]
        assert result.context["scene_info"]["stats"]["performance"]["fps"] == 60
        assert result.context["user_settings"]["preferences"]["theme"] == "dark"
        assert result.context["history"][1]["action"] == "modify"

    def test_update_result(self):
        """Test updating an existing result."""
        # Create an initial result
        result = ActionResultModel(
            success=True, message="Initial message", prompt="Initial prompt", context={"initial": "value"}
        )

        # Update the result
        result_dict = result.model_dump()
        result_dict["message"] = "Updated message"
        result_dict["context"] = {**result.context, "updated": "new_value"}
        updated_result = ActionResultModel(**result_dict)

        # Verify the updated result
        assert updated_result.success is True
        assert updated_result.message == "Updated message"
        assert updated_result.prompt == "Initial prompt"
        assert updated_result.context["initial"] == "value"
        assert updated_result.context["updated"] == "new_value"
