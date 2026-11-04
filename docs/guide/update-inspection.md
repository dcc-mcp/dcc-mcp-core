# Read-only update inspection

`dcc-mcp-cli --output json update inspect` reports version observations and
official release evidence without applying or staging an update, starting a
gateway, refreshing marketplace state, or changing installation pins. Use
`--offline` to avoid network requests.

This command is available only in builds that include the inspection entry
point. Older CLIs can activate staged updates before parsing arguments; do not
run an older binary with `--help`, `--version`, or `update check` merely to test
whether this read-only command exists.

## Evidence boundaries

The schema distinguishes the package manager's recorded installed version, the
executing CLI runtime version, and Core compiled into that CLI. A package-manager
receipt may lag behind a self-updated executable. None of these observations
proves the Core version loaded in a separate gateway or DCC process. Missing
observations carry a null version and an explicit reason.

The official release provider reports source identity, retrieval time, cache
age/TTL, offline status, release URL, and published time when available. A
network failure or stale cache is not an up-to-date result. Existing gateway
cache metadata does not establish an immutable official release or fix relation.

Stable is the initial supported channel. Unknown channels, unobserved pins,
and absent host/runtime compatibility metadata remain explicit. A latest
release or matching platform asset alone is insufficient evidence of host
compatibility. Inspection never changes a pin or upgrades a package.

For a requested official issue, an explicit issue-closing reference in release
notes is reported as a release declaration. It is not proof that a particular
running process is fixed. Vague changelog similarity, a closed issue without
release linkage, or an unreleased merged PR cannot establish a fixed version.

## Action boundary

The report is a dry run. Review its source/target observations, compatibility
and pin gaps, official links, and upgrade/rollback guidance before obtaining
approval for a concrete change. The existing verified updater remains the
installation owner. Do not equate clearing a staged update with downgrading an
already activated binary, and do not infer a rollback capability from internal
failure-recovery code.

The executable's early dispatch gate parses the same Clap command definition
before the legacy startup path. Inspection and parse errors/help/version return
there. Other valid commands retain their existing startup behavior. Library
callers must use the inspection service rather than `presentation::cli::run`
when they require this read-only boundary.

## Existing mechanisms and follow-up batches

- Adapter/application discovery continues to use the signed installation
  catalog and its revision, digest, expiry, withdrawal, and Core bounds.
- Skill/plugin discovery continues to use marketplace sources and targets.
  Catalog support, local installation, live registration, and compatibility are
  separate observations.
- Diagnostic drafts use the existing feedback route/bundle/file pipeline and
  its repository, fingerprint, content-digest, and catalog authorization binding.
  Free-form metadata must not be treated as safe merely because it passed a
  pattern redactor or carries a `public_safe` label.

Companion agent instructions live in `dcc-mcp/dcc-mcp-agent-plugins`, under the
default Skill's marketplace maintenance references. This first interface does
not claim full host inventory, all-provider offline caching, or fleet upgrade
support.

## Official reference models

The boundary between environment metadata and runtime observations follows the
same distinction made visible by the versioned
[pip inspect report](https://pip.pypa.io/en/stable/reference/inspect-report/).
Package pins are constraints to preserve, as illustrated by
[WinGet pinning](https://learn.microsoft.com/en-us/windows/package-manager/winget/pinning).
Release identity and publication fields come from the
[GitHub releases API](https://docs.github.com/en/rest/releases/releases);
they do not independently prove runtime compatibility or successful remediation.
