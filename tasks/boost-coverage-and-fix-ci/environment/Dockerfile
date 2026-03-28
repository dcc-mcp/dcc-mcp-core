FROM python:3.12-slim

# System dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create workspace
WORKDIR /workspace

# Clone the repository at the starting point (main branch)
ARG REPO_URL=https://github.com/loonghao/dcc-mcp-core.git
RUN git clone ${REPO_URL} /workspace && \
    git fetch origin feat/skill-system

# Install project and dev dependencies
RUN pip install --no-cache-dir -e ".[yaml]" && \
    pip install --no-cache-dir \
        pytest \
        pytest-cov \
        pytest-mock \
        pytest-asyncio \
        pyfakefs \
        nox \
        ruff \
        isort \
        pre-commit

# Create log directories for verifier
RUN mkdir -p /logs/verifier /logs/agent

# Copy task files
COPY tests/ /task/tests/
COPY solution/ /task/solution/

ENTRYPOINT ["/bin/bash"]
