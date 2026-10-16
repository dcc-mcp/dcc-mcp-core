# Adapter release-index pilot

Status: **opt-in, review-only CLI distribution tooling**. The production catalog,
installer, update notifier and Marketplace behavior are unchanged. This pilot
reduces manual version/URL/hash transcription without making a new upstream
release an automatic installation approval.

## Existing ownership and the missing link

Adapter repositories already own package version, wheel metadata, Python and Core
dependencies, entrypoints, and release-specific installation instructions. PyPI
serves their versioned release metadata and content-addressed wheel URLs. Some
adapters also package host compatibility matrices. These are useful existing
release manifests; a second per-repository hand-maintained hash file would add
maintenance.

Core's [verified installation catalog](verified-install-catalog.md) remains the
only official aggregate writer and signer. Its preparation stage already checks
PyPI identity, yanking, artifact digest/size, wheel metadata, archive safety and
release-commit instructions. The CLI already verifies the fixed feed's Sigstore
identity, expiry and rollback history, atomically preserves the verified envelope,
and keeps the selected artifact fixed through planning and execution.

The missing link is **discovery and promotion**, not a second installer. The new
[release index](../../compatibility/adapter-release-index.json) stores only stable bindings between an
existing approved catalog entry, its official repository and its PyPI project.
The two-source pilot covers OpenUSD and FreeCAD. No version, asset URL or hash
needs to be copied into this index when an adapter publishes a new release.

## Run the candidate compiler

From a Core checkout, with Python and PyYAML available:

```sh
python scripts/ci/prepare_release_candidates.py --output candidates.json
```

JSON catalog input requires no third-party Python modules; the existing YAML
loader uses PyYAML. The compiler uses the publisher's bounded HTTP transport,
official-host redirect restrictions and GitHub credential scoping. It never
imports an adapter or starts a host, daemon, gateway or installer. Output paths
are explicit and atomically replaced after successful validation. It does not
write any runtime cache or edit its catalog/index inputs.

The report follows
[`adapter-release-candidate-v1.schema.json`](../../contracts/adapter-release-candidate-v1.schema.json).
Its `authority` is always `candidate_only`. Each candidate records:

- Observation time, approved catalog/index digests and per-entry baseline digest
- Whether the version was explicitly selected or merely observed as latest
- Exact wheel version, URL, SHA-256, byte count and `py3-none-any` package tag
- Release tag resolved to a full source commit and commit-pinned instructions
- Released wheel `Requires-Python`, all `Requires-Dist`, entrypoints and any
  indexed compatibility JSON, bound to the verified archive bytes
- Existing Core-bound policy evaluation, promotion blockers and a field diff
- A PyPI provenance reference explicitly marked `verification: not_checked`

The provenance URL is a reference, not evidence that a bundle exists or that its
signature has been verified. Resolving the repository tag to a commit does not
cryptographically prove that the wheel was built from that commit. The existing
Core signer remains the official CLI
trust authority. Adding a PyPI attestation verifier is a separate policy change.

Universal wheel tags describe the adapter package, not the external host or
optional native libraries. `host_status` and `native_extras_status` remain
`not_checked`. A FreeCAD compatibility matrix is a released declaration, not a
probe of an installed FreeCAD. OpenUSD's `usd-core` extra still needs compatible
native wheels before native USD capabilities can be claimed.

## Explicit selection and review

To investigate a particular release, avoid a floating latest selection:

```sh
python scripts/ci/prepare_release_candidates.py \
  --select dcc-mcp-openusd==0.11.0 \
  --select dcc-mcp-freecad==0.9.1 \
  --output candidates.json
```

For **unblocked** explicit selections only, `--catalog-output selected.json`
exports the existing `{ "version": "1", "entries": [...] }` catalog shape. The
export contains only indexed entries, not a replacement for the full ecosystem
catalog. This operator-supplied unsigned file is not the signed official feed.
Existing CLI `--catalog` commands must label it as explicit provenance. Never
install merely because the file exists or a report says `review_required`.

The export gate refuses observed-latest selections, downgrades, prohibited
installation policy, policy reasons needing review, missing/ambiguous/conditional
Core requirements, or bounds rejected by the existing one-minor-line policy.
Core dependency identity follows PyPA name normalization, so equivalent spelling
variants cannot hide duplicate declarations or direct-reference requirements.
For this pilot, Core requirement clauses must use complete three-component
versions; abbreviated compatible-release specifiers, wildcards, prereleases,
direct URLs and malformed requirement syntax are blocked rather than interpreted
by a permissive policy parser. It does not invent tighter compatibility bounds.
Old catalog entries carrying
only `min_core_version` continue through the existing publisher unchanged;
this opt-in gate does not retroactively invalidate that legacy path.

On October 13, 2026, the published OpenUSD 0.11.0 and FreeCAD 0.9.1 wheels both
used a `<1.0.0` Core ceiling. That is wider than the current Core compatibility
contract. The pilot therefore discovers their exact release evidence but blocks
catalog export. Adapter owners must publish accurate tested bounds before these
candidates can be promoted through this gate. Green upstream CI does not remove
that contract mismatch. The [recorded public observation](../../tests/fixtures/release-index/public-observation-2026-10-13.json)
contains the actual hashes, source commits and declared compatibility evidence;
it is a historical regression fixture, not a live status or offline trust cache.

After review, the selected values can be proposed in the curated catalog. The
existing `prepare_install_catalog.py` revalidates all release evidence before the
existing protected workflow signs and publishes the aggregate. Review the final
source commit and artifact digest against the candidate receipt: a GitHub tag can
move, and a rerun is a new observation, not permission to substitute bytes. No
workflow, automatic promotion policy, signing key, credential or permission is
introduced by this pilot.

A failed network/identity/hash/archive check fails the invocation and retains the
previous output. It never turns an unavailable upstream into "latest". Previous
candidate reports are historical observations; they are not verified offline
installation receipts and must not be used to bypass expiry or revocation checks.

## Follow-on consumer/cache contract

The following is **design scope**, not implemented runtime behavior:

1. Keep [the existing catalog module](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/crates/dcc-mcp-cli/src/application/install/catalog.rs) as the single CLI owner of fixed-feed retrieval,
   verification, atomic persistence and rollback checks. The release index is a
   publisher-side input; clients should fetch one aggregated signed snapshot,
   not fan out to every adapter repository on every invocation.
2. Add an internal cache-observation record bound to the envelope digest and
   fixed source identity. Record ETag, last successful check, last attempt and
   next retry separately from the signed payload's issue/expiry times.
3. Use conditional GET against the existing transport. A `304` can establish a
   current online check only when the exact retained envelope is reverified and
   unexpired. An ETag by itself is never authenticity or rollback evidence.
4. TTL/backoff governs opportunistic background discovery. TTL cache hits and
   offline reads remain `latest_checked: false`; failed/disabled checks are
   unknown. Explicit online installation retains its fresh-verification gate.
   Failures back off; concurrent refreshes and notices deduplicate. Active jobs
   are never interrupted or upgraded by discovery.
5. Preserve atomic replacement, authenticated rollback history and fail-closed
   integrity errors. Recheck state under the existing cache lock before commit.
   A future `304` test must cover cache replacement by another process, missing
   cache bytes, expiry, source mismatch and digest mismatch.
6. Version public response schemas before exposing checked-at/age/backoff fields.
   Existing discovery-v1 and install-SOP-v2 provenance definitions are closed
   schemas and must not gain undeclared fields. Old CLIs continue consuming the
   existing signed schema-v1 aggregate, independent of how it was prepared.
7. Reuse the catalog model and attestation engine already shared with Marketplace.
   Keep adapter software/platform/install semantics distinct from Skill package
   dependency/interface semantics. A future shared transport/cache abstraction
   must be extracted from a proven owner rather than introducing a third HTTP,
   cache or signing-policy stack. Marketplace behavior stays unchanged here.

A future per-repository CI exporter can serialize the same evidence after the
existing package release job, using its existing trust identity. Its descriptor
must pin the release commit, exact artifact digest and compatibility declarations;
mutable `latest` pointers are discovery hints only. Central automatic promotion
requires a separately reviewed allowlist and compatibility policy in the existing
sole publisher. Studio config precedence, batch deployment/version locks and Rez
integration remain separate design work.
