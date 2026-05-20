# Testing Matrix

Use this to choose the smallest useful validation for a DCC-MCP adapter skill.

## Instruction-Only Skills

- `validate_skill(skill_dir)` should be clean.
- Reference links in `SKILL.md` should point to existing files.
- Search terms should make the skill discoverable without competing with
  runtime domain skills.

## Tool-Backed Skills

- Validate the skill directory.
- Parse `SKILL.md` and confirm every tool has `execution` and `affinity`.
- Confirm every `source_file` exists.
- Import scripts without the DCC installed when possible.
- Unit test pure argument validation, path normalization, and result envelopes.
- Mock host APIs for deterministic scene-independent behavior.

## Adapter Integration

Add one route through the adapter layer when behavior crosses discovery,
loading, or dispatch:

- `scan_and_load` or `scan_and_load_lenient` for path and metadata behavior.
- `create_skill_server(..., McpHttpConfig(...))` for server registration.
- `tools/list` and `tools/call` for MCP-visible behavior.
- Gateway `POST /v1/search`, `/v1/describe`, `/v1/call`, or `/v1/call_batch`
  for REST-visible behavior.

## Live Host Coverage

Use live DCC tests for behavior mocks cannot prove:

- Host version probing.
- Main-thread dispatch.
- Plugin loading.
- Scene mutation and undo-sensitive work.
- UI screenshot, selection, or active document state.

Skip gracefully when the host is unavailable. The test should explain what was
not exercised.

## VRS Coverage

Add a Verified Regression Suite trace when the regression is only visible
through HTTP or gateway routing:

- Put one concern per JSONL trace.
- Use `expect_any` when legitimate transport outcomes differ.
- Use `skip_preflight` for traces that require a live optional DCC.
- Update `tests/vrs/README.md` with the trace purpose.

## Pre-PR Checklist

- Skill validates cleanly.
- Tool metadata has explicit execution, affinity, schema, and safety hints.
- DCC API imports are lazy.
- Main-thread-only work is guarded by metadata and host-side checks.
- Unit tests cover script behavior.
- At least one adapter or gateway path covers discovery/load/call when relevant.
- Docs or llms indexes mention new public guidance when agent behavior changes.
