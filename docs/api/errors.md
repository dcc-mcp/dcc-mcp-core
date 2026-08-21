# Errors

**Module:** `dcc_mcp_core.errors`  
**Exported symbol:** `DccMcpError`

## DccMcpError

```python
from dcc_mcp_core import DccMcpError
```

`DccMcpError` is the shared base class for public Python exceptions raised by
`dcc-mcp-core`. Catch it when a boundary needs to handle any library-owned
failure without also swallowing unrelated application exceptions.

```python
try:
    descriptor.validate()
except DccMcpError as exc:
    report_adapter_failure(str(exc))
```

Specialized errors preserve their previous built-in categories through
multiple inheritance. For example, `AssetImportValidationError` remains a
`ValueError`, `AssetSyncConflictError` remains a `RuntimeError`, and
`ScriptExecutionSerializationError` remains a `TypeError`.

The Python exception root is import-light and does not load the native
extension. It is separate from the Rust `dcc_mcp_models::DccMcpError` enum,
which classifies failures inside the Rust workspace.
