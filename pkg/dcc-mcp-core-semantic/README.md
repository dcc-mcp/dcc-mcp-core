# dcc-mcp-core-semantic

Native Rust semantic embeddings for [dcc-mcp-core](https://github.com/loonghao/dcc-mcp-core),
shipped as a separate PyPI wheel so the main `dcc-mcp-core` install stays
free of ONNX Runtime and the ~25-40 MB wheel size that comes with it.

## When to install this

Only when you actually need dense semantic recall for skill / capability
search. The default `pip install dcc-mcp-core` install ships with a
`HashedEmbedder` (zero-dep, hashing-trick + character n-grams) which is
enough for ≤100-skill DCC adapters and tolerates morphology variants
like `render` / `rendering` already.

If your skill catalogue grows to many hundreds of skills, or your agents
ask in natural language that does not share token structure with your
SKILL.md metadata, install this companion wheel to upgrade
`OnnxEmbedder` to true dense semantic recall.

## How to install

```bash
pip install 'dcc-mcp-core[semantic]'
```

This pulls in `dcc-mcp-core-semantic` (this package) via the `[semantic]`
extra, plus `fastembed` as a Python-side fallback for platforms where the
Rust wheel is not yet available.

You can also install this package directly if you want only the Rust
backend without the Python fallback:

```bash
pip install dcc-mcp-core dcc-mcp-core-semantic
```

## How it gets used

Your adapter code does not change. Once installed,
`dcc_mcp_core.OnnxEmbedder()` automatically prefers the Rust extension:

```python
from dcc_mcp_core import OnnxEmbedder, VectorSkillIndex

# Loads the BAAI/bge-small-en-v1.5 model on first use, cached to
# ~/.cache/fastembed/ (or wherever DCC_MCP_EMBED_MODEL_DIR points).
emb = OnnxEmbedder()
idx = VectorSkillIndex(embedder=emb)
```

## Configuration

Both env vars are honoured by `OnnxEmbedder` regardless of which backend
serves the call:

| Variable | Default | Purpose |
|----------|---------|---------|
| `DCC_MCP_EMBED_MODEL` | `BAAI/bge-small-en-v1.5` | HuggingFace model name. Must be one of `dcc_mcp_core_semantic.native.SUPPORTED_MODELS`. |
| `DCC_MCP_EMBED_MODEL_DIR` | unset (fastembed default) | On-disk cache for the ONNX model bytes. Pre-place this on a shared mount for firewalled studios. |

## Build from source

The source lives in the main `dcc-mcp-core` repository, NOT here.
Rust crate: `crates/dcc-mcp-semantic/`. Wheel build:

```bash
cd pkg/dcc-mcp-core-semantic
maturin build --release
```

ONNX Runtime is pulled in at build time via the `ort` crate's
`download-binaries` strategy. The wheel that maturin produces is
self-contained — end users do not need to install ONNX Runtime separately.

## License

MIT, matching `dcc-mcp-core`.
