# Bound Service Registry Schema Evolution

Status: Accepted

## Context

`ServiceEntry` evolved through optional fields and a special normalization path
for legacy gateway sentinels. That preserved old rows but provided no boundary
for a future incompatible change. An older gateway could deserialize only the
fields it understood and later overwrite a newer `services.json`, silently
discarding data or semantics.

## Decision

Every newly written `ServiceEntry` carries `schema_version = 1`. Rows that omit
the field are legacy version 0 and remain readable. File-registry readers inspect
the raw version before deserializing a row and reject every version newer than
`SERVICE_ENTRY_SCHEMA_VERSION` with
`UnsupportedServiceEntrySchemaVersion`.

An unsupported future schema is not corruption. The registry leaves the source
file untouched and fails initialization or mutation, preventing an older writer
from replacing it. Invalid JSON or invalid field types remain corruption and use
the existing quarantine behavior.

Programmatically supplied entries pass the same version check before registration.
The Python `ServiceEntry` projection and `to_dict()` expose the version so adapter
diagnostics can report the contract in use.

## Consequences

- Legacy registry files continue to load without migration.
- Current writers publish an explicit, inspectable schema contract.
- A future incompatible writer fails safely against older processes instead of
  losing registry data.
- A schema bump requires an intentional reader/writer rollout and a documented
  compatibility decision.

## Alternatives considered

- Continuing to rely only on `serde(default)` was rejected because it cannot
  identify an incompatible future row.
- Quarantining unknown versions was rejected because a valid newer file is not
  corrupt and must not be moved aside.
- Silently skipping newer rows was rejected because the next write would erase
  them from the shared registry.
