# ADR-021: Use content-addressed revisions for cross-DCC asset sync

## Status

Accepted

## Context

An agent may need to move evolving scene data between local DCC applications
and remote or local consumers. A raw filesystem path is not a portable wire
contract, and exposing arbitrary source or destination paths would grant more
local access than the operation requires. Direct last-writer-wins copying also
loses provenance and can silently overwrite concurrent edits.

## Decision

Core defines a host-neutral `AssetSyncRevision` manifest and a local
`FileAssetSyncStore` reference implementation.

- Payloads are immutable SHA-256-addressed objects.
- A `(channel_id, asset_id)` head advances monotonically.
- Publishers provide `expected_head_revision`; mismatches fail with
  `AssetSyncConflictError` while an OS-backed per-head lock prevents two local
  writers from publishing the same revision.
- Manifests carry an `artefact://sha256/...` reference and never serialize a
  workstation path.
- Consumers materialize a revision only beneath an operator-owned root. A
  public tool may select a validated relative subfolder, never the root.
- DCC adapters own format support, source-root policy, size limits, host import
  or refresh behavior, and any mapping into their native canvas or scene.

This contract transports files and revisions. Tunnel, relay, authentication,
and subscription transports remain separate layers and may carry the same
manifest without changing it.

## Consequences

- Local producer and consumer adapters can prove byte identity and provenance.
- Bidirectional flows use the same optimistic revision rule in both directions
  instead of overwriting each other silently.
- Core stays independent of Houdini, ComfyUI, Blender, and their native APIs.
- The initial store keeps one current head per asset and immutable objects. A
  future remote store may add retention, garbage collection, subscriptions,
  resumable transfer, and authorization without changing the revision shape.

## Alternatives considered

- Pass absolute paths between tools: rejected because paths leak workstation
  layout and do not cross host boundaries.
- Put ComfyUI or Houdini import logic in Core: rejected because native host
  lifecycle and supported formats belong to adapters.
- Last-writer-wins file copying: rejected because it hides concurrent edits and
  provides no stable revision or digest for verification.
