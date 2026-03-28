# dcc-mcp-core development commands
# Usage: vx just <recipe>

# Cross-platform shell configuration
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
set shell := ["sh", "-cu"]

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
[unix]
clean:
    rm -rf dist build *.egg-info .nox .coverage coverage.xml

[windows]
clean:
    if (Test-Path dist) { Remove-Item -Recurse -Force dist }
    if (Test-Path build) { Remove-Item -Recurse -Force build }
    if (Test-Path .nox) { Remove-Item -Recurse -Force .nox }
    Get-ChildItem -Filter *.egg-info -Directory | Remove-Item -Recurse -Force
    Remove-Item -ErrorAction SilentlyContinue -Force .coverage, coverage.xml
