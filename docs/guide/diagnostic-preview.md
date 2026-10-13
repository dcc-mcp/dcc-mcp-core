# Preview diagnostics and issue content

Use an explicitly selected CLI build containing the read-only entry point.
Older binaries can activate a staged update before argument parsing; do not
probe them with help, version, or preview commands to establish safety.

```bash
dcc-mcp-cli --output json feedback bundle reviewed-finding.json
dcc-mcp-cli --output json feedback file reviewed-finding.json
```

Both preview paths finish before legacy updater activation, profile/gateway
startup, and background maintenance. `feedback file` without `--yes` may read
the routed official GitHub issue tracker to check duplicates, but never creates
an issue or comment. Invalid authorization arguments fail at the early gate.

## Diagnostic collection

`dcc-mcp.feedback-bundle.v2` is a minimal diagnostic summary. Its `finding`
value is not a publishable Finding v1 and cannot replace the original reviewed
Finding used by `feedback file`.

The default reads only the explicitly selected Finding. An install report can
be selected with `--install-report`. Host log collection requires an explicit
`--log-dir`; existing path, byte, line, and structured-event restrictions still
apply. The preview does not run doctor, probe interpreters, query a gateway,
discover log directories, or collect raw environment variables.

Projection retains typed release versions, recognized public component names,
loading-source categories, fixed status/error classes, and bounded numeric or
boolean fields. Unknown fields and arbitrary text are omitted. Version build
metadata is removed, and unrecognized private prerelease labels are omitted.
Nested values use the same allowlist. Omission summaries contain fixed category
counts, never discarded keys or values.

`redaction_status.review_required` is true and `secret_free_guarantee` is false.
These fields state the limitation explicitly: an allowlist does not replace
review of the material intended for publication. Uncollected sources stay
explicitly unavailable or not applicable.

## Exact issue review

A plan includes duplicate candidates plus `preview.title`, `preview.body`,
`preview.comment_body`, and `preview.document_sha256`. Duplicate lookup sends
the fingerprint and fixed phase categories; arbitrary tool slugs, error text,
and DCC labels are not sent as keyword queries. Lookup failure is an error,
not evidence that no duplicate exists.

Review the exact title/body or comment, repository, and target issue before
authorizing a write. The emitted next-step command binds the existing Finding
path/digest, fingerprint, repository, and catalog path/digest, plus
`--expected-document-sha256`. The digest covers all three exact draft strings.
Content or template changes invalidate old authorization and require a fresh
plan. Existing duplicate/race checks still run before submission.

Plans may contain local paths needed for authorization. Do not publish the
entire plan or attach diagnostic bundles automatically. This workflow neither
uploads files nor grants approval to submit an issue.

Use [update inspection](update-inspection.md) for release/fix evidence and
[ecosystem discovery](ecosystem-discovery.md) for official catalog metadata.
