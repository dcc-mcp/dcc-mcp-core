"""Example demonstrating the parameter validation system in DCC-MCP-Core.

This example shows how to use the parameter validation system to validate and convert
parameters for action functions, including type conversion, validation, and error handling.
"""

# Import built-in modules
from typing import Any
from typing import Dict
from typing import List
from typing import Optional

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.parameter_groups import ParameterGroup
from dcc_mcp_core.parameter_groups import with_parameter_groups
from dcc_mcp_core.parameter_models import with_parameter_validation


# Example 1: Basic parameter validation with type conversion
@with_parameter_validation
def create_user(name: str, age: int, email: str, is_active: bool = True) -> Dict[str, Any]:
    """Create a new user with the given parameters.

    Args:
        name: The user's full name
        age: The user's age in years
        email: The user's email address
        is_active: Whether the user account is active

    Returns:
        Dictionary containing the created user information

    """
    # This function will automatically validate and convert parameters
    # For example, if age is passed as a string, it will be converted to an int
    # If is_active is passed as a string 'true', it will be converted to a boolean

    user = {
        "id": 12345,  # In a real system, this would be generated
        "name": name,
        "age": age,
        "email": email,
        "is_active": is_active,
    }

    return ActionResultModel(
        success=True,
        message=f"Created user {name}",
        prompt="You can now add roles or permissions to this user",
        context={"user": user},
    )


# Example 2: Parameter groups and dependencies
@with_parameter_validation
@with_parameter_groups(
    ParameterGroup(
        "identification", "User identification parameters", ["username", "email"], required=True, exclusive=True
    ),
    ParameterGroup("authentication", "Authentication parameters", ["password", "token"], required=True, exclusive=True),
    # Define parameter dependencies
    password=("username", None, "Password requires a username"),
)
def authenticate_user(
    username: Optional[str] = None,
    email: Optional[str] = None,
    password: Optional[str] = None,
    token: Optional[str] = None,
    remember_me: bool = False,
) -> Dict[str, Any]:
    """Authenticate a user with either username/password or email/token.

    Args:
        username: The user's username
        email: The user's email address
        password: The user's password (required if username is provided)
        token: Authentication token (can be used instead of password)
        remember_me: Whether to keep the user logged in

    Returns:
        Dictionary containing authentication result

    """
    # This function demonstrates parameter groups and dependencies
    # The user must provide either username or email (but not both)
    # The user must provide either password or token (but not both)
    # If username is provided, password must also be provided

    # In a real system, this would verify credentials against a database
    auth_method = "username/password" if username else "email/token"

    return ActionResultModel(
        success=True,
        message=f"User authenticated via {auth_method}",
        prompt="You can now access protected resources or update user settings",
        context={
            "auth_method": auth_method,
            "user_id": 12345,
            "session_expires": "2023-12-31T23:59:59Z" if remember_me else "2023-12-01T23:59:59Z",
        },
    )


# Example 3: Complex parameter types and validation
@with_parameter_validation
def search_products(
    query: str,
    categories: Optional[List[str]] = None,
    price_range: Optional[Dict[str, float]] = None,
    sort_by: str = "relevance",
    page: int = 1,
    page_size: int = 20,
) -> Dict[str, Any]:
    """Search for products matching the given criteria.

    Args:
        query: Search query string
        categories: List of category names to filter by
        price_range: Dictionary with 'min' and 'max' price values
        sort_by: Field to sort results by (relevance, price, rating)
        page: Page number for pagination
        page_size: Number of results per page

    Returns:
        Dictionary containing search results

    """
    # This function demonstrates validation of complex parameter types
    # categories will be converted from string to list if needed
    # price_range will be validated to ensure it has the correct structure

    # In a real system, this would query a database or API
    results = [
        {"id": 1, "name": "Product 1", "price": 19.99, "category": "Electronics"},
        {"id": 2, "name": "Product 2", "price": 29.99, "category": "Home & Kitchen"},
        {"id": 3, "name": "Product 3", "price": 9.99, "category": "Books"},
    ]

    # Filter by categories if provided
    if categories:
        results = [p for p in results if p["category"] in categories]

    # Filter by price range if provided
    if price_range:
        min_price = price_range.get("min", 0)
        max_price = price_range.get("max", float("inf"))
        results = [p for p in results if min_price <= p["price"] <= max_price]

    return ActionResultModel(
        success=True,
        message=f"Found {len(results)} products matching '{query}'",
        prompt="You can refine your search or view product details",
        context={
            "results": results,
            "total": len(results),
            "page": page,
            "page_size": page_size,
            "filters_applied": {"categories": categories, "price_range": price_range, "sort_by": sort_by},
        },
    )


def run_examples():
    """Run the parameter validation examples."""
    print("\n=== Example 1: Basic Parameter Validation ===\n")

    # Valid parameters
    result1 = create_user("John Doe", 30, "john@example.com")
    print(f"Result 1 (valid parameters): {result1}\n")

    # Type conversion
    result2 = create_user("Jane Smith", "25", "jane@example.com", "true")
    print(f"Result 2 (type conversion): {result2}\n")

    # Invalid parameters
    try:
        result3 = create_user("Bob", "not-a-number", "invalid-email")
        print(f"Result 3 (invalid parameters): {result3}\n")
    except Exception as e:
        print(f"Result 3 (invalid parameters): Error - {e}\n")

    print("\n=== Example 2: Parameter Groups and Dependencies ===\n")

    # Valid: username + password
    result4 = authenticate_user(username="user123", password="securepass")
    print(f"Result 4 (username + password): {result4}\n")

    # Valid: email + token
    result5 = authenticate_user(email="user@example.com", token="abc123", remember_me=True)
    print(f"Result 5 (email + token): {result5}\n")

    # Invalid: missing required group
    result6 = authenticate_user(username="user123")
    print(f"Result 6 (missing password): {result6}\n")

    # Invalid: mutually exclusive parameters
    result7 = authenticate_user(username="user123", email="user@example.com", password="pass")
    print(f"Result 7 (username + email): {result7}\n")

    print("\n=== Example 3: Complex Parameter Types ===\n")

    # Valid parameters with complex types
    result8 = search_products(
        query="laptop", categories=["Electronics", "Computers"], price_range={"min": 500, "max": 2000}, sort_by="price"
    )
    print(f"Result 8 (complex types): {result8}\n")

    # String to list conversion
    result9 = search_products(
        query="furniture", categories="Home & Kitchen, Furniture", price_range='{"min": 100, "max": 500}'
    )
    print(f"Result 9 (string to complex types): {result9}\n")


if __name__ == "__main__":
    run_examples()
