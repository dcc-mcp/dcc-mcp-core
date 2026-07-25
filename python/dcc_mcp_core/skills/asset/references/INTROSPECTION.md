# INTROSPECTION.md — asset

> The AssetDescriptor contract and cross-DCC asset pipeline.

## AssetDescriptor

Canonical wire format from `dcc_mcp_core.asset_import`:

```python
@dataclass
class AssetDescriptor:
    name: str           # Canonical name
    asset_type: str     # model, texture, material, rig, animation
    display_name: str   # Human-readable
    variants: list[AssetFileVariant]
    attribution: AssetAttribution
    metadata: dict
```

## AssetFileVariant

```python
@dataclass
class AssetFileVariant:
    format: str     # fbx, usd, abc, obj, gltf, png, exr...
    path: str       # Absolute or catalog-relative
    file_size: int
    lod: str        # proxy, low, medium, high
```

## AssetAttribution

```python
@dataclass
class AssetAttribution:
    author: str
    source: str     # Source DCC or tool
    version: str    # Semantic version
    license: str    # SPDX identifier
```

## Pipeline Stages

```
source → resolve → import → scene
                 ↓
         export ← scene
```

Each stage validates the descriptor before proceeding.
