# Official design provider trial

Experimental source trial on Core 0.20.41 for Figma, Penpot, Pencil, Sketch
and Framer. It reuses official tools/APIs and an existing authorized session.
It is not released adapter discovery or native-host acceptance. See the
[official capability matrix](../../docs/guide/design-provider-bridge.md).
See the [native acceptance prerequisites and current blockers](NATIVE_ACCEPTANCE.md)
before choosing an existing host; all five native branches remain unrun.

## Run against an existing host

Use a Python environment containing the candidate Core source and, for MCP,
the optional official client SDK from `requirements.txt` (Python >=3.10).
The code never installs dependencies. Do not run account setup, grant OAuth,
generate keys, or purchase a plan as part of this trial.

Discovery is the default operation:

```shell
python runner.py --provider figma --url http://127.0.0.1:3845/mcp
python runner.py --provider sketch --url http://localhost:31126/mcp
```

Figma's local route inspects the desktop host; it does not supply remote
`use_figma` writing. Sketch must run on the native Mac host. Penpot's local
official endpoint is supplied by its existing running server; use
`--provider penpot --url <official-loopback-url>`. Do not pass credential URLs.
For Pencil, supply the existing executable and arguments from its official
installed stdio client entry:

```shell
python runner.py --provider pencil --command <existing-official-executable> --arg <argument>
```

Framer uses its existing official SDK package and a project-bound key already
supplied by the operator's secret provider. The key is not a CLI argument,
output field, or configuration file created by this trial:

```shell
python runner.py --provider framer --project <existing-authorized-project-id>
```

The curated Framer API facade advertises actual SDK methods, not an official
MCP catalog. It supports project/canvas/node inspection and native frame
creation; no publish/deploy operation is included. The official External
Agent bridge may instead be used through its already authorized host.

To perform one requested operation, first inspect the exact live descriptor,
then use `--call <observed-name> --arguments-file <local-json-object>`. The
operation is invoked once without retry. Outputs can contain private project
data; keep them local and review before sharing. A successful response still
requires independent structural/persistence/export verification.

## Core discovery facade

Add `--serve` to keep the same authorized session bound to a loopback Core
service. A temporary registry isolates the default trial. `--registry-dir`
selects an operator-approved registry for existing gateway discovery; the
trial does not ensure/restart a machine gateway.

The service advertises `design_session__status`, `capabilities`, `refresh`,
`describe` and `call`. Refresh the official catalog before every edit; the
long-lived trial does not install vendor notification callbacks. Refresh
advances generation; `describe`/`call` require the current generation. Official
names, schemas, annotations and results remain available in the Core JSON
envelope. The outer MCP wire is a facade, not native content-block passthrough.

Figma remote and Penpot remote should use a caller-owned authorized session:

```python
from dcc_mcp_core.design_bridge import DesignSessionBridge

bridge = DesignSessionBridge("figma")
catalogue = await bridge.connect(existing_authorized_session)
# Select an observed tool and its schema, then invoke the requested operation.
result = await bridge.call_tool(tool_name, arguments, expected_generation=catalogue["generation"])
private_response = result.to_dict()
bridge.disconnect()
```

The caller owns initialization, host eligibility, existing authorization,
document targeting and transport cleanup. The bridge does not log in or store
credentials.

## Repeatable assets and validation

Use the single [component library to frontend pilot](pilot/README.md) for all
five providers. Native read/write/save-reopen/export evidence remains pending
until an authorized native host is exercised. Host-free tests explicitly use
contract fixtures and never count as vendor acceptance.

```shell
python -m pytest tests/test_design_bridge.py tests/test_design_provider_facade.py tests/test_design_provider_example_contracts.py
```

Run that test command from the Core repository root. The HTTP test uses the
real Core server and official client SDK with a deterministic fixture upstream.
The SDK is optional in the Core test dependency set; absence skips that test.
The example contracts also run through the existing `pytest tests/` CI gate
when Node is already available. They use a fake SDK fixture and do not contact
Framer. The native and lite Python 3.7 smoke lists include the bridge import;
local grammar checks do not replace execution of those CI jobs.
