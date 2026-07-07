# Python 3.7 CI image

`dcc-mcp-core` uses a canonical Linux image for the supported native
CPython 3.7 lane:

```text
ghcr.io/dcc-mcp/dcc-mcp-py37@sha256:1dd8fba0da3f2a70ce44f55aee7cf08ee3d67768367deb3985a4f70779687ac5
```

The image is built from `.github/docker/py37-ci/Dockerfile` and must be
referenced by digest in workflows. Tags are only publishing conveniences; CI
must not depend on mutable tags.

## Contents

- Python `3.7.17` from the pinned manylinux2014 base image.
- `pip>=20.3,<24.1`, `setuptools`, `wheel`, `build`, and `maturin`.
- Rust/Cargo with `clippy` and `rustfmt`.

## CI contract

- `.github/workflows/ci.yml` builds and tests the native Linux
  `cp37-cp37m` wheel as the default Python 3.7 gate.
- `.github/workflows/build-wheels.yml` uploads the release Python 3.7 Linux
  wheel from the native lane.
- The `py37-lite` pure-Python wheel remains a fallback-only job. It must keep
  proving that `_core` is absent and import fallback paths still work, but it
  must not be the default supported Maya Python 3.7 artifact.

## Boundaries

- The GHCR package must either be public or grant GitHub Actions access to
  every repository that pulls it. For the current cross-repo gate that means at
  least `dcc-mcp-core` and `dcc-mcp-maya`.
- This image covers Linux CI only. Native macOS and Windows `cp37` wheels need
  separate runner strategies because they must be built on their target OS.
- DCC host smoke tests still need real host environments such as Maya, Blender,
  or mayapy containers. The Linux Python 3.7 image proves wheel build and
  resolver behavior, not host integration.
- When the image is rebuilt, publish it to GHCR, read back the immutable
  digest, update every workflow reference, and include a smoke proving:
  `python == 3.7.17`, `pip`, `maturin`, `rustc`, and `cargo` are available.
