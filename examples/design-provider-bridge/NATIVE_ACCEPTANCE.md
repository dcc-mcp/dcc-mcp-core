# Native acceptance prerequisites and current blockers

All five provider branches have `native_acceptance=not_run`. No successful
fixture, static asset validation or official server documentation response
is a native read/write/save-reopen/export pass.

The project supplies an original [Pencil input](pilot/native/fieldkit.pen)
and [shared component/token specification](pilot/design-spec.json). Once an
authorized host is available, create an isolated test document or copy through
the observed official route; no user-supplied production design file is needed.

## Observed execution environment

The inspected environment is Windows. A read-only check found no exact
Figma, Pencil, Penpot, Framer or Sketch process names and no matching vendor
directories in the inspected standard Program Files roots. Pencil/pen/Framer
commands were not resolved on the current PATH. These are scoped observations,
not proof that every per-user or IDE installation is absent.

Node 24.19.0 is present. `framer-api` could not resolve from this example,
and the example process had no Framer key/project environment entries.
Only presence was checked; credential values were not read or copied.

The earlier Figma desktop loopback probe found no listener. A subsequent
system listener inventory was denied and stopped without another inspection
route. Browser sessions were not inspected because the supported browser
control execution entry was unavailable. Existing web login state therefore
remains unknown. No native session is currently confirmed usable.

## Exact existing objects needed

| Provider | Existing objects required | Smallest next operator action |
| --- | --- | --- |
| Figma desktop read | Installed, signed-in Figma desktop; Dev or Full seat on a paid plan; accessible Design file and frame/node | Open the chosen existing file, enter Dev Mode, enable its desktop MCP server and confirm `http://127.0.0.1:3845/mcp`. This route reads and does not provide general canvas writes |
| Figma write | Already authorized remote MCP connection in an eligible Catalog client; Full seat and target-file edit permission; exact file URL and component/token targets | Identify that existing authorized host and designated test page. If missing, ask specifically for the Figma remote OAuth grant/client eligibility; do not substitute installation of the chat plugin |
| Penpot local | Already installed/running official single-user server at `http://localhost:4401/mcp`; local plugin manifest at `http://localhost:4400/manifest.json`; existing logged-in editable file/page; one connected plugin | Identify the existing server and open the existing local plugin in the chosen file. A new install or Chromium local-network grant requires a separate decision |
| Penpot remote | Existing enabled account MCP route and valid key already configured in a host; chosen logged-in file connected to MCP | Identify the already configured host and target file/page. Do not pass a token URL in chat; key creation/regeneration is a separate credential action |
| Pencil / pen.dev | Existing desktop/IDE host and open `.pen`; its installed official MCP executable and non-secret arguments | Identify the existing command/argument entry and open a test copy of our supplied `fieldkit.pen`. Do not read secret environment fields. There is no documented universal Windows executable or named-pipe path to guess |
| Pencil headless alternative | Already installed `@pen.dev/cli`, Node >=22.19 and existing CLI authentication | Use the existing `pen` executable. Desktop login does not prove CLI authentication; `pen interactive` is a tool shell, not the MCP JSON-RPC stdio entry |
| Sketch | Supported Mac, non-App-Store Sketch >=2025.2.4, existing license and enabled local MCP, chosen `.sketch` copy | Run the trial on that Mac at `http://localhost:31126/mcp`. Windows is a native platform blocker; no remote port/trust change is implied |
| Framer | Existing resolvable `framer-api`, project URL/ID approved for test edits, project-bound key supplied by its existing owner | Identify the existing SDK environment/project and have its owner use the existing secret provider. New API key creation or External Agent browser grant requires a separate decision |

Conditional no-new-auth reads are possible through an existing Figma desktop
session, Penpot single-user local host/plugin, Pencil official host, Sketch
Mac host or Framer existing project key. None of those sessions was established
in this execution environment. Penpot `tools/list`, `high_level_overview` and
`penpot_api_info` can succeed without a connected file; they establish only
official-server/schema documentation evidence.

## Minimal real pilot sequence

Refresh and describe the live catalog, select the existing test document and
read tokens/components first. Check exact target identity immediately before
each edit; Penpot must assert document/page identity inside the same official
script. A stale connection or a browser focus change cannot select the target.

1. **Figma:** use the already authorized remote `use_figma` tool to edit the
   designated test page, then read back returned IDs. Wait for cloud saved
   status, reopen the same file URL and reread those identities. Validate
   actual `download_assets` outputs. A `.fig` local-copy import creates new
   file/component identities and is a separate backup check.
2. **Penpot:** inspect Plugin API context and perform a bounded `execute_code`
   edit after asserting file/page. Read back the result, wait for saved status,
   reopen that same file/page and reconnect the plugin. Use observed
   `export_shape` PNG/SVG schema and verify real output files. Native `.penpot`
   download/reimport is separate from same-file cloud persistence.
3. **Pencil:** read live tools/`read_skill` and existing `.pen`, edit a designated
   copy, use a verified host save route (desktop/IDE Save or headless `save()`),
   close/reopen output and reread
   component instances/token variables. For an existing headless CLI route,
   `pen interactive --in <existing.pen> --out <test-copy.pen>` supports the
   official tool shell and `save()`. Check actual export artifacts; zero exit
   status alone does not prove successful export.
   For this original input, change the accent to `#245C78` and only the
   `asset-tidal/card-title` instance override to `Tidal native check`; verify
   the other instances/master, native IDs and references after reopening.
4. **Sketch:** on the native Mac, inspect document/layers, edit a designated
   copy using observed `run_code`, wait for `document.save` callback, reopen
   with `Document.open` and reread native IDs. Export via supported
   `sketch.export` and inspect real PNG/SVG/JSON files separately from the
   native `.sketch` file saved by `document.save`.
5. **Framer:** connect the existing project, inspect project/root and create
   one frame under the approved parent using `createFrameNode`. Read it back,
   disconnect, connect anew to the same project and reread the returned ID.
   The current five-method facade has no export method. Determine an observed
   official image-export route before completing export; JSON responses and
   publish/deploy cannot satisfy that stage. SDK failure is non-transactional
   and may follow a partial edit: preserve an indeterminate outcome, reconnect
   and read back instead of replaying.

After actual native read/export, derive the shared frontend specification
from verified native results. The committed frontend currently comes from
local authored JSON. Check browser interactions separately before claiming
an accepted component-library-to-frontend workflow.

## First-party references

- Figma: [desktop setup](https://developers.figma.com/docs/figma-mcp-server/local-server-installation/), [seat requirements](https://help.figma.com/hc/en-us/articles/32132100833559-Guide-to-the-Figma-MCP-server), [write requirements](https://developers.figma.com/docs/figma-mcp-server/write-to-canvas/), [remote eligibility](https://developers.figma.com/docs/figma-mcp-server/remote-server-installation/).
- Penpot: [setup](https://help.penpot.app/mcp/), [local endpoints](https://raw.githubusercontent.com/penpot/penpot/main/mcp/README.md), [single-user authentication](https://raw.githubusercontent.com/penpot/penpot/main/mcp/packages/server/src/PluginBridge.ts), [native export/import](https://help.penpot.app/user-guide/export-import/export-import-files/).
- Pencil: [AI integration](https://docs.pencil.dev/getting-started/ai-integration), [installation](https://docs.pencil.dev/getting-started/installation), [native files/save](https://docs.pencil.dev/core-concepts/pen-files), [headless CLI](https://docs.pencil.dev/for-developers/pen-cli).
- Sketch: [official MCP](https://www.sketch.com/docs/mcp-server/), [native API](https://developer.sketch.com/reference/api/).
- Framer: [Server API setup](https://www.framer.com/developers/server-api-quick-start), [frame creation](https://www.framer.com/developers/reference/plugins-create-frame-node), [node readback](https://www.framer.com/developers/reference/plugins-get-node), [non-transactional API and export boundary](https://www.framer.com/developers/server-api-faq).
- Persistence evidence: [Figma local-copy identity rules](https://help.figma.com/hc/en-us/articles/8403626871063-Save-a-local-copy-of-files), [Figma autosave/history](https://help.figma.com/hc/en-us/articles/360038006754-View-a-file-s-version-history), [Penpot saved status](https://help.penpot.app/user-guide/first-steps/the-interface/).
