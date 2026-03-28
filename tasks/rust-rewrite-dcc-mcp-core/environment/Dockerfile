FROM rust:1.94-bookworm

# Install Python 3.11 + pip
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3.11 python3.11-venv python3.11-dev python3-pip \
    git curl jq \
    && rm -rf /var/lib/apt/lists/*

# Set Python 3.11 as default
RUN update-alternatives --install /usr/bin/python3 python3 /usr/bin/python3.11 1 && \
    update-alternatives --install /usr/bin/python python /usr/bin/python3.11 1

# Install Rust components
RUN rustup component add clippy rustfmt

# Install maturin, just, pytest
RUN pip install --break-system-packages maturin pytest pytest-cov

# Install just
RUN curl --proto '=https' --tlsv1.2 -sSf https://just.systems/install.sh | bash -s -- --to /usr/local/bin

# Clone the repo at v0.10.0 tag (starting point)
RUN git clone --depth 1 --branch v0.10.0 https://github.com/loonghao/dcc-mcp-core.git /app

WORKDIR /app

# Create required directories
RUN mkdir -p /logs/verifier /logs/agent /solution /tests
