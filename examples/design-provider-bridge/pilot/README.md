# Shared design-provider pilot

Fieldkit is one authored, editable component library and collection-page fixture
shared by Figma, Penpot, Pencil/pen.dev, Sketch, and Framer. It includes actual
frontend source and a schema-derived Pencil document candidate. Its six sample
collection entries and item-count labels are content fixtures, not delivered
material packs or six completed showcases.

No provider host session, account login, OAuth grant, installation, paid plan,
native export, or browser interaction was performed by this example. The recorded
verification covers saved local artifacts only.

## Run with an existing Python interpreter

From this directory:

```console
python scripts/build.py
python scripts/build.py --check
python scripts/validate.py --record
```

The scripts use Python 3.7+ standard library only. They install no dependencies,
start no services, access no credentials, and make no network requests. The build
writes only inside this pilot directory. Direct Python is intentional: this
host-free example uses an already available interpreter without a tool manager
installing or changing a runtime.

Open `frontend/index.html` in an authorized local browser to try the page. It has
no external fonts, images, libraries, or fetch requests. A browser runtime has not
been used for the recorded checks.

1. Select a card to inspect its content.
2. Search or filter by category, including the empty-result state.
3. Change the page title and accent. The primary button automatically chooses
   black or white text for contrast with the edited accent.
4. Use **Export page** to download the complete editable JSON specification.
5. Place that export in this directory as `fieldkit-edited.json`, then rebuild:

```console
python scripts/build.py --spec fieldkit-edited.json
python scripts/validate.py --record
python scripts/build.py --spec fieldkit-edited.json --check
```

Edits are held in browser memory until export. They do not persist to a provider
document, upload to any account, or replace `design-spec.json` automatically. Run
the first command without `--spec` to restore the committed fixture.

## Editable deliverables

| File | Purpose | Acceptance scope |
| --- | --- | --- |
| `design-spec.json` | Typed tokens, six reusable component definitions, collection-page content | Authored local source |
| `templates/styles.css`, `templates/app.js` | Editable layout, responsive styles, and interactions | Source, no browser runtime acceptance |
| `frontend/index.html`, `frontend/styles.css`, `frontend/app.js` | Runnable offline page generated from the library | Local artifacts |
| `frontend/design-spec.json` | Complete saved input for the current generated page | Local JSON save/reopen |
| `native/fieldkit.pen` | Pencil 2.20 candidate with three reusable components, nested refs, and six card instances | Documented subset static check; native unverified |
| `artifacts.json` | Relative artifact paths and SHA-256 digests | Reproducible local export |
| `local-verification.json` | Recorded local checks with explicit pending boundaries | Local artifacts only |
| `evidence-template.json` | Four native stages per provider, initially pending | No native evidence yet |

The frontend reuses card, tag, filter, navigation, and metadata components. Its
component styles resolve the token references from the JSON library; those
references also become named Pencil variables. Layout-only CSS and illustration
colors remain editable source CSS.

## Pencil candidate boundary

The official [Pencil format documentation](https://docs.pencil.dev/for-developers/the-pen-format)
publishes the 2.20 document schema, variable bindings, reusable nodes, refs, and
descendant overrides. This candidate uses a small documented subset. Our local
checker verifies that subset, IDs, variables, and references; it is not the
official editor engine or a full TypeScript-schema validator.

The document intentionally covers three of the six frontend component families
and simplifies the card artwork to editable swatches. It is a native input
candidate, not proof of full visual parity. Official-tool validation must still
open, edit, save, reopen, and export it. The
[official CLI](https://docs.pencil.dev/for-developers/pen-cli) is a possible native
validation path after device, installation, and authentication authority exist.
Use official tools for future edits and preserve the original versioned file.

## Verification

`validate.py` reads the actual saved files and checks SHA-256 digests, embedded
input parity, repeated component instances, unique element IDs, CSS token
resolution, and the absence of external page resources. It also performs a real
local JSON write/reopen/rebuild with changed title and accent, checks deterministic
generation, rejects invalid references and duplicate IDs, and confirms the
native evidence template has no fabricated passes.

JavaScript syntax can additionally be checked with an existing Node runtime:

```console
node --check frontend/app.js
```

Syntax and static checks do not verify browser interactions or native provider
capabilities. Follow [WORKFLOW.md](WORKFLOW.md) to collect those separately before
the project is presented as an accepted showcase or Marketplace result.
