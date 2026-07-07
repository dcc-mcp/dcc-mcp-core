FROM quay.io/pypa/manylinux2014_x86_64@sha256:fc58a97ebc50df8acfa25204711c27aa5a89c818cba270ec780feae8c0f58268

ARG RUST_VERSION=1.95.0

ENV CARGO_HOME=/opt/rust/cargo \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PY37_HOME=/opt/python/cp37-cp37m \
    RUSTUP_HOME=/opt/rust/rustup
ENV PATH="${PY37_HOME}/bin:${CARGO_HOME}/bin:${PATH}"

RUN ln -sf "${PY37_HOME}/bin/python" /usr/local/bin/python \
    && ln -sf "${PY37_HOME}/bin/python" /usr/local/bin/python3 \
    && ln -sf "${PY37_HOME}/bin/python" /usr/local/bin/python3.7 \
    && ln -sf "${PY37_HOME}/bin/pip" /usr/local/bin/pip \
    && python --version | grep "Python 3.7.17" \
    && python -m pip install --upgrade \
        "pip>=20.3,<24.1" \
        "setuptools==68.0.0" \
        "wheel==0.42.0" \
        "build==1.1.1" \
        "maturin>=1.0,<2.0"

RUN curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain "${RUST_VERSION}" \
    && rustup component add clippy rustfmt \
    && chmod -R a+rX /opt/rust \
    && python --version \
    && pip --version \
    && maturin --version \
    && rustc --version \
    && cargo --version

LABEL org.opencontainers.image.title="dcc-mcp Python 3.7 CI image" \
      org.opencontainers.image.description="Pinned Linux CI image for native cp37 dcc-mcp wheel builds." \
      org.opencontainers.image.source="https://github.com/dcc-mcp/dcc-mcp-core"
