# dcc-mcp-core development commands
# Usage: vx just <recipe>

# Default recipe - show available commands
default:
    @just --list

# Run linter checks
lint:
    vx uvx nox -s lint

# Run linter with auto-fix
lint-fix:
    vx uvx nox -s lint-fix

# Run tests
test:
    vx uvx nox -s pytest

# Run tests with verbose output
test-v:
    vx uvx nox -s pytest -- -xvs

# Run a specific test file
test-file file:
    vx uvx nox -s pytest -- {{ file }} -v

# Install pre-commit hooks via prek
prek-install:
    vx prek install

# Run pre-commit hooks on all files
prek-all:
    vx prek --all-files

# Run pre-commit hooks on staged files
prek:
    vx prek

# Validate pre-commit config
prek-validate:
    vx prek validate-config

# Install project dependencies
install:
    vx uv sync

# Build the project
build:
    vx uv build

# Clean build artifacts
clean:
    rm -rf dist build *.egg-info .nox .coverage coverage.xml
