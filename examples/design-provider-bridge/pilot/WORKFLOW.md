# Component library to page to frontend

This is a bounded hand-off recipe for the shared pilot, not a new provider tool
implementation or an installable public skill. The public DCC-MCP router owns
discovery and provider-specific guidance. Keep upstream tool names, input schemas,
authentication requirements, and write behavior intact.

## Local preparation

Build the fixture and record its local validation report as described in
`README.md`. Use the same project ID and canonical specification hash for every
provider branch. A provider document should preserve token bindings and component
relationships rather than merely reproduce a screenshot.

Copy `evidence-template.json` to `evidence-run.json` for an actual acceptance run.
Never edit the template to pretend a host ran. Keep private document identifiers
and account/session evidence local; redact them before public PR or Marketplace
review. Retain sanitized request IDs, relative artifact paths, and hashes.

## Provider preflight

1. Inventory existing DCC-MCP instances and use one targeted discovery search.
2. Follow the returned load/describe/call step. Check the connected official
   provider's actual current tool/API schemas and read/write capabilities.
3. Select an existing authorized test document or project. Do not create an
   account, buy a plan, grant OAuth, make a persistent credential, install a
   plugin, or change network/device trust as part of this recipe.
4. If access is missing, record the concrete blocker and pause the dependent
   native steps. A local candidate cannot replace missing host acceptance.

Sketch requires an available supported native Mac host. Pencil requires the
official editor or CLI to validate its candidate. For the other providers, use
their verified official MCP/API connection; do not assume that frontend export,
component creation, or persistence exists just because a transport connects.

## Native acceptance stages

| Stage | Action | Evidence needed to pass |
| --- | --- | --- |
| `read` | Read the selected document, existing tokens, and component structure | Actual official response and sanitized document/version reference |
| `write` | Add the shared library/page or make a bounded title/accent edit through official tools | Actual write result, component IDs, and a subsequent read confirming the changed value |
| `save_reopen` | Persist with the provider's documented lifecycle, close/reopen or start a fresh read session | Reopened title/token value and preserved instance-to-component links; an in-memory read alone is insufficient |
| `export` | Export through a supported official operation into an editable/native or documented interchange artifact | Actual artifact, relative path, format, digest, and a reopen/import check where supported |

Use `passed` only after collecting the stage's evidence. Use `blocked`, `failed`,
or `unsupported` with a redacted reason when the observed host or API cannot
complete it. Pending and unsupported are not passes. For automatic-persistence
providers, prove persistence with a fresh document/session read rather than
inventing a save command. Preserve returned job IDs across timeouts; query actual
status before repeating a mutation.

For Pencil, the generated candidate contains three reusable component families.
Official native acceptance must check its nested overrides and token variables;
the remaining frontend families require actual native implementation if full
page parity is part of the requested acceptance scope.

## Frontend hand-off and review

After actual native export, retain the provider artifact and derive the shared
specification from the provider's verified read/export results. Rebuild and run
local validation; record any mapping loss. The committed frontend originates
from the authored local JSON, so it currently proves only the local half of this
workflow. It is not a provider-generated export.

In an authorized browser, verify selection, keyboard focus, category/search
filtering, empty results, title/accent edits, JSON download, and a rebuild from
that download. Check narrow and wide layouts and accent contrast. Record this as
browser evidence independently of static syntax checks.

Keep this as one project in the shared Showcase/Marketplace plan. Native files,
read/write traces, save/reopen evidence, frontend source, hashes, license, and
documented limitations make it reviewable. A provider count, artwork count, or
screen capture count does not multiply completed showcases. Do not publish a
Marketplace package, create an additional repository, or release software from
this example recipe.
