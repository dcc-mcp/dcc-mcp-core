"""Tests for the Action base class with Pydantic models."""

# Import built-in modules
from typing import ClassVar
from typing import List
from typing import Optional

# Import third-party modules
from pydantic import Field
from pydantic import ValidationError
from pydantic import field_validator
from pydantic import model_validator
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel


class TestActionWithPydanticModels:
    """Tests for the Action base class with Pydantic models."""

    class CreateSphereAction(Action):
        """Test action class for creating a sphere."""

        name = "create_sphere"
        description = "Create a sphere in the scene"
        tags: ClassVar[List[str]] = ["geometry", "create"]
        dcc = "test_dcc"

        class InputModel(Action.InputModel):
            """Input model for CreateSphereAction."""

            radius: float = Field(1.0, description="Radius of the sphere")
            segments: int = Field(8, description="Number of segments")
            position: List[float] = Field([0, 0, 0], description="Position of the sphere")
            name: Optional[str] = Field(None, description="Name of the sphere")

            @field_validator("radius")
            def validate_radius(cls, v):
                """Validate that radius is positive."""
                if v <= 0:
                    raise ValueError("Radius must be positive")
                return v

            @field_validator("segments")
            def validate_segments(cls, v):
                """Validate that segments is at least 3."""
                if v < 3:
                    raise ValueError("Segments must be at least 3")
                return v

            @model_validator(mode="after")
            def validate_model(self):
                """Validate model dependencies."""
                # If name is provided, position must not be origin
                if self.name and self.position == [0, 0, 0]:
                    raise ValueError("Position must not be origin when name is specified")
                return self

        class OutputModel(Action.OutputModel):
            """Output model for CreateSphereAction."""

            object_name: str = Field(description="Name of the created sphere")
            position: List[float] = Field(description="Final position of the sphere")
            radius: float = Field(description="Final radius of the sphere")
            prompt: Optional[str] = Field(None, description="Suggestion for next steps")

        def _execute(self):
            """Execute the action."""
            # Simulate creating a sphere
            object_name = self.input.name or f"sphere_{id(self)}"

            # Set the output
            self.output = self.OutputModel(
                object_name=object_name,
                position=self.input.position,
                radius=self.input.radius,
                prompt=f"You can now modify {object_name} using the modify_sphere action.",
            )

    def test_action_initialization(self):
        """Test action initialization."""
        # Create action with context
        context = {"scene": "test_scene", "user": "test_user"}
        action = self.CreateSphereAction(context=context)

        # Verify initialization
        assert action.input is None
        assert action.output is None
        assert action.context == context

    def test_action_setup_with_valid_input(self):
        """Test action setup with valid input."""
        # Create and setup action
        action = self.CreateSphereAction().setup(radius=2.0, segments=12, position=[1, 2, 3], name="test_sphere")

        # Verify setup
        assert action.input is not None
        assert action.input.radius == 2.0
        assert action.input.segments == 12
        assert action.input.position == [1, 2, 3]
        assert action.input.name == "test_sphere"

    def test_action_setup_with_default_values(self):
        """Test action setup with default values."""
        # Create and setup action with minimal parameters
        action = self.CreateSphereAction().setup()

        # Verify default values
        assert action.input.radius == 1.0
        assert action.input.segments == 8
        assert action.input.position == [0, 0, 0]
        assert action.input.name is None

    def test_action_setup_with_invalid_radius(self):
        """Test action setup with invalid radius."""
        # Try to setup action with invalid radius
        with pytest.raises(ValidationError) as exc_info:
            self.CreateSphereAction().setup(radius=-1.0)

        # Verify error message
        assert "Radius must be positive" in str(exc_info.value)

    def test_action_setup_with_invalid_segments(self):
        """Test action setup with invalid segments."""
        # Try to setup action with invalid segments
        with pytest.raises(ValidationError) as exc_info:
            self.CreateSphereAction().setup(segments=2)

        # Verify error message
        assert "Segments must be at least 3" in str(exc_info.value)

    def test_action_setup_with_invalid_dependencies(self):
        """Test action setup with invalid dependencies."""
        # Try to setup action with invalid dependencies
        with pytest.raises(ValidationError) as exc_info:
            self.CreateSphereAction().setup(name="test_sphere", position=[0, 0, 0])

        # Verify error message
        assert "Position must not be origin when name is specified" in str(exc_info.value)

    def test_action_process_success(self):
        """Test successful action processing."""
        # Create, setup, and process action
        action = self.CreateSphereAction().setup(radius=2.0, segments=12, position=[1, 2, 3], name="test_sphere")
        result = action.process()

        # Verify result
        assert isinstance(result, ActionResultModel)
        assert result.success is True
        assert "Successfully executed create_sphere" in result.message
        assert "You can now modify test_sphere" in result.prompt
        assert result.context["object_name"] == "test_sphere"
        assert result.context["position"] == [1, 2, 3]
        assert result.context["radius"] == 2.0

    def test_action_process_with_exception(self):
        """Test action processing with exception."""

        # Create a subclass that raises an exception in _execute
        class FailingAction(self.CreateSphereAction):
            def _execute(self):
                raise RuntimeError("Test error")

        # Create, setup, and process action
        action = FailingAction().setup()
        result = action.process()

        # Verify result
        assert isinstance(result, ActionResultModel)
        assert result.success is False
        assert "Failed to execute create_sphere" in result.message
        assert result.error == "Test error"
        assert "traceback" in result.context

    def test_create_parameter_model(self):
        """Test creating a parameter model dynamically."""
        params_model = Action.create_parameter_model(
            radius=(float, Field(default=1.0, description="Radius of the sphere")),
            position=(List[float], Field(default=[0, 0, 0], description="Position of the sphere")),
            name=(Optional[str], Field(default=None, description="Name of the sphere")),
        )

        # Create an instance of the model
        params = params_model(radius=2.0, position=[1, 2, 3])

        # Verify the model
        assert params.radius == 2.0
        assert params.position == [1, 2, 3]
        assert params.name is None

    def test_process_parameters_dict(self):
        """Test processing parameters as dictionary."""
        # Process parameters as dictionary
        params = {"radius": "2.0", "segments": "12", "position": [1, 2, 3], "enabled": "true"}
        processed = Action.process_parameters(params)

        # Verify processed parameters
        assert processed == params

    def test_process_parameters_string(self):
        """Test processing parameters as string."""
        # Process parameters as string
        params_str = "radius=2.0, segments=12, enabled=true, name=test"
        processed = Action.process_parameters(params_str)

        # Verify processed parameters
        assert processed["radius"] == 2.0
        assert processed["segments"] == 12
        assert processed["enabled"] is True
        assert processed["name"] == "test"

    def test_process_parameter_value(self):
        """Test processing individual parameter values."""
        # Test various parameter values
        assert Action.process_parameter_value("true") is True
        assert Action.process_parameter_value("false") is False
        assert Action.process_parameter_value("none") is None
        assert Action.process_parameter_value("123") == 123
        assert Action.process_parameter_value("3.14") == 3.14
        assert Action.process_parameter_value("test") == "test"
        assert Action.process_parameter_value([1, 2, 3]) == [1, 2, 3]  # Non-string values unchanged
