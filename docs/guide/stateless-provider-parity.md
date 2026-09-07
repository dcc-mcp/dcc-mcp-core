# Stateless resource and prompt provider parity

The opt-in 2026 route uses the existing `RegistryContext` providers for
`resources/list`, `resources/read`, `prompts/list`, and `prompts/get`. It does
not maintain a second registry, reinterpret resource URIs as file paths, or
invoke a different prompt renderer. The same loaded skill/catalog state backs
both transport paths.

Discovery advertises resources/prompts only when the feature is enabled and
its provider is registered. It does not advertise subscriptions or list-change
notifications; consumers re-list to observe catalog changes.

- Listing is sorted by resource URI or prompt name and paginated at 64 entries.
  Follow `nextCursor`; malformed, non-ASCII or out-of-range cursors are rejected.
- Resource payloads retain their text/blob and MIME type. Prompt templates
  retain their declared arguments and source metadata.
- Missing providers or disabled features return HTTP 404 with method-not-found. Missing
  targets and malformed arguments return invalid-params; a provider's disabled
  resource and internal-error categories remain distinct. Responses do not
  expose provider exception text. Business/provider errors remain HTTP 200;
  malformed request envelopes fail at ingress before provider access.
- Prompt arguments must be a string-to-string object; no implicit value
  stringification occurs on this route.
- Pagination does not create a session or pin a catalog snapshot. Re-list after
  loading/unloading skills rather than reusing old cursors indefinitely.

Every modern provider request supplies the required namespaced request metadata
and matching standard headers. Successful results pass through the shared final
response builder: server identity lives in result `_meta`, `resultType` is
`complete`, and cacheable resource reads/lists and prompt lists use conservative
`ttlMs: 0` and `cacheScope: private`. Prompt rendering is not marked cacheable.
Legacy responses retain their existing envelope.

This repairs provider wiring and its request/response integration. Subscriptions
and notifications have separate gates; successful provider tests are not a
claim of full protocol support. Adapter and public skill authoring
APIs are unchanged, so no public agent-plugins skill change is needed for this
provider repair.

Regressions cover provider contracts in Rust and actual HTTP calls through
installed Python wheels for both Blender and Maya fixture skill catalogs.
No licensed DCC application is required for these transport tests.
