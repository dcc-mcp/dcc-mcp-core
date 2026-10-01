# Native Inkscape example

This example creates editable vector documents through a typed DCC-MCP skill
and an actual Inkscape-hosted inkex effect. The controller writes JSON requests
and extension configuration. It does not write SVG. Inkscape creates the blank
document, invokes the effect, commits its stdout document, and saves the SVG.
PNG, Plain SVG, PDF, and text-to-path conversion use native Inkscape actions.

The runtime is a **standalone controller with a host-invoked native extension**.
It is not an embedded GUI adapter or a released, catalog-supported Inkscape
integration. The service has no fake GUI host PID; each native operation owns
one separate application process and uses a distinct `dccmcp_` app-id tag.
An explicitly configured non-default gateway and isolated registry are
required. No shared gateway is started, restarted, or reconfigured.

## Run

Install a verified Inkscape 1.4 portable distribution separately. Its inkex
extension dependencies must be available to Inkscape. This example never
downloads software, changes security settings, or installs system fonts.

Configure these process-local values with absolute paths:

```powershell
$env:DCC_MCP_INKSCAPE_EXE = 'C:\task\inkscape\bin\inkscape.exe'
$env:DCC_MCP_INKSCAPE_WORKSPACE = 'C:\task\output'
$env:DCC_MCP_INKSCAPE_REGISTRY_DIR = 'C:\task\registry'
$env:DCC_MCP_INKSCAPE_GATEWAY_PORT = '19765'
# Optional: application-private font directories, separated by the OS path separator.
$env:DCC_MCP_INKSCAPE_FONT_DIRS = 'C:\task\fonts'
python server.py
```

The operator starts an isolated official gateway separately with the same
registry and port. Use `dcc-mcp-cli list` against that task gateway, search for
`native vector document`, follow the returned load/describe step, and call
the exact returned slug. Do not guess an instance identifier. Keep one agent
session ID and `--require-gateway` on measured calls. Direct instance MCP calls
can prove protocol execution but do not establish gateway statistics coverage.

The Python environment needs `dcc-mcp-core>=0.20`. Scripts use the standard
declarative subprocess execution path and the example's process-local
`PYTHONPATH`; they do not introduce another script executor or UI automation.
The isolated Inkscape profile is under `<workspace>/.inkscape-mcp/profile`.
An optional private fontconfig includes the portable application's default
configuration and adds the supplied font directories. Its task cache precedes
the included configuration's caches. A requested SVG font
family alone is not proof of successful font resolution; validate the actual
renderer or fontconfig match before attributing a typeface to final artwork.

## Vector plan

```json
{
  "canvas": {"width": 256, "height": 256, "view_box": "0 0 256 256"},
  "nodes": [
    {"type": "layer", "id": "artwork", "label": "Editable artwork"},
    {"type": "path", "id": "triangle", "parent": "artwork",
     "d": "M 32,224 L 128,32 L 224,224 Z", "fill": "currentColor"},
    {"type": "text", "id": "caption", "parent": "artwork",
     "x": 128, "y": 244, "text": "VECTOR", "font_family": "sans-serif",
     "font_size": 12, "font_weight": 700, "text_anchor": "middle"}
  ]
}
```

Supported nodes are layers, groups, paths, rectangles, circles, ellipses, text,
and linear gradients with ordered stops. A child's parent must be an earlier
layer or group. Gradients are native definitions and can be referenced by
`url(#gradient-id)`. Paint accepts local gradients, `currentColor`, `none`,
and literal colors. The protocol rejects arbitrary SVG, XML, scripts, external
resources, CSS injection, nonfinite values, and unknown fields. Plans are
bounded to 2 MB, 5000 objects, and a 32768 px canvas.

`build_schema.py` is an authoring helper. Its explicit schemas are committed as
`skills/inkscape-vector/tools.yaml`; runtime discovery does not execute it.
The plan validator normalizes compatibility forms internally, while the
advertised client schema stays explicit and simple.

## Evidence and acceptance

Every build records the invocation nonce, native process ID, effect process ID,
parent executable, `SELF_CALL`, object count, exact argv, software diagnostics,
and output hash. The controller checks the evidence before atomically publishing
a new output. Existing outputs are never overwritten. These are local pipeline
provenance checks, not cryptographic host attestation. Windows GLib spawn helper
behavior must pass the actual live test; a mock test is not a native host proof.
Build and export operations preserve `host.json` under their invocation directory
even when the host fails or evidence validation rejects the output. A failed
parent-image lookup is recorded in `effect.json` and does not bypass verification.

`document_inspect` reopens the saved file with Inkscape and queries native
geometry. `document_open` launches a separate GUI process and returns its PID
for the official exact-process `ui-control` workflow. Core's dedicated
`python -m dcc_mcp_core.ui_control_server` accepts that process ID and the actual
document window handle; its independent launcher PID is not the GUI host PID.
The native controller keeps its five subprocess tools and does not install an
in-process UI bridge. It does not capture the desktop
or claim visual acceptance. A locked desktop can still allow headless creation
and export, but GUI inspection remains incomplete until the desktop is available.

ICO and ICNS require a separate packaging step using the exported PNGs. Small
icons should use separately authored simplified vector plans. Exporting the
same large composition at a smaller size is not optical simplification.

The parent-image provenance implementation currently supports Windows and Linux
(`/proc`). macOS native execution has not been validated. Creating a valid ICNS
container does not establish macOS adapter support. Native operations are
monolithic; Core job cancellation does not automatically cancel the host. A
120-second operation timeout kills only the owned native process.

## Validation

```powershell
python -m pytest tests/test_native_inkscape.py -q
# Explicit opt-in for the real host regression:
$env:DCC_MCP_INKSCAPE_LIVE_TEST = '1'
$env:DCC_MCP_INKSCAPE_EXE = 'C:\task\inkscape\bin\inkscape.exe'
python -m pytest tests/test_native_inkscape.py -q
```

Unit tests cover protocol validation, schema agreement, injection rejection,
path containment, process ownership, provenance failures, and publication races.
The opt-in live test creates a native editable document, reopens it, and checks
an actual 32 px PNG header. Run it on the target portable version before treating
this example as validated host capability.

References: [Inkscape CLI](https://wiki.inkscape.org/wiki/Using_the_Command_Line),
[script extension protocol](https://wiki.inkscape.org/wiki/Script_extensions),
[official native action wiring](https://gitlab.com/inkscape/inkscape/-/blob/master/src/actions/actions-effect.cpp),
[inkex](https://inkscape.gitlab.io/extensions/documentation/).
