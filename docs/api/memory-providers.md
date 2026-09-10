# Memory providers

`dcc_mcp_core.runtime.memory_provider` is the vendor-neutral boundary for durable
memory integrations. It complements the existing session-oriented
`agent_memory` module.

```python
from dcc_mcp_core import (
    LocalMemoryProvider,
    MemoryCoordinator,
    MemoryRecallRequest,
)

provider = LocalMemoryProvider(".dcc-mcp/memory.sqlite")  # explicit opt-in
memory = MemoryCoordinator(provider, timeout_secs=1.0)
future = memory.recall(MemoryRecallRequest(query="scene units", scope="project"))
# Await/result handling belongs off the DCC main thread.
```

External distributions can expose a factory without changing Core:

```toml
[project.entry-points."dcc_mcp.memory_providers"]
my-memory = "my_package:make_provider"
```

Discover factories with `discover_memory_provider_factories()`. Loading is
explicit; discovery never selects a provider or enables remote persistence.

Provider records are JSON-safe, inert context. They cannot execute a tool,
grant approval, broaden a DCC target, or bypass lifecycle policy. Apply consent,
sensitivity, scope, TTL, and retention policy before `remember`.
