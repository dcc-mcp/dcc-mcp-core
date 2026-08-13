# Cross-DCC Asset Sync

Use `AssetSyncRevision` and `FileAssetSyncStore` when an adapter must publish
an evolving file for another DCC without exposing arbitrary local paths.

```python
from dcc_mcp_core import FileAssetSyncStore

store = FileAssetSyncStore(operator_sync_root)
revision = store.publish(
    exported_file,
    channel_id="houdini-comfyui",
    asset_id="hero-mesh",
    format="obj",
    mime="model/obj",
    expected_head_revision=0,
    source_instance_id=houdini_instance_id,
)
consumer_path = store.materialize(
    revision,
    operator_owned_consumer_root,
    subfolder="3d",
)
```

The manifest returned by `revision.to_dict()` contains a content digest,
monotonic revision, optional source instance and base revision, metadata, and a
path-free artefact reference. `expected_head_revision=0` means the asset must
not exist yet. For an update, pass the revision previously read from
`read_head()`; a stale value raises `AssetSyncConflictError`.

## Adapter boundary

An adapter tool should accept a relative source name and resolve it beneath an
operator-configured source root. The destination root must also come from
operator configuration. Validate the supported format and size before calling
`publish()`, then materialize beneath the consumer's native import directory.

Do not expose absolute `source_path`, `target_root`, or a generic destination
subfolder as agent-controlled public inputs. Do not put host-specific import,
canvas, refresh, or watch logic in Core.

For a remote path, transfer the immutable object through an authenticated
artefact or tunnel transport and keep the same revision manifest. Transport
reconnect, subscriptions, retention, and conflict UI are independent of this
storage contract.
