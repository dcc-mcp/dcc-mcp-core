# Skill Ownership Policy

> **Status**: Accepted
> **Applies to**: All first-party adapter repositories that ship multiple skill packages
> **Decision**: 2026-05-13

## Policy Statement

Each common file operation type exposed through bundled adapter skills **MUST** have a **single primary owning package**.

## Rationale

- **Consistency**: Users get predictable behavior regardless of which skill package they use
- **Maintainability**: Clear ownership avoids schema divergence and duplicate logic
- **Discoverability**: Agents can reliably identify the canonical implementation
- **Quality**: Focused ownership enables deeper testing and specialization

## Rules

### 1. Primary Owner

- Each user-facing file operation (e.g., `read_file`, `write_file`, `list_directory`) MUST be implemented in exactly one primary skill package per repository
- The primary owner MUST:
  - Define the canonical input/output schemas
  - Implement the core logic
  - Own tests and documentation
  - Set the behavioral contract

### 2. Secondary Skills

Secondary skills MAY link or thin-wrap the primary owner but MUST NOT:

- Duplicate schemas with divergent field definitions
- Re-implement core logic with behavioral differences
- Introduce conflicting argument names or semantics

Allowed secondary patterns:

```python
# Thin wrapper delegating to primary owner
def read_file(path: str, **kwargs):
    """Secondary skill wrapping primary owner."""
    from primary_package import read_file as _primary_read
    return _primary_read(path, **kwargs)
```

```python
# Skill that links to primary (e.g., symlink or re-export)
# skills/secondary-skill/SKILL.md
---
name: secondary-read
links:
  - ../primary-skill
---
```

### 3. Multi-Package Repositories

Repositories shipping multiple skill packages (e.g., `dcc-mcp-skills-filesystem`, `dcc-mcp-skills-git`) MUST:

- Designate one package as primary for each operation type
- Document ownership in `SKILL_OWNERSHIP.yml` at repo root:
  ```yaml
  ownership:
    read_file:
      primary: dcc-mcp-skills-filesystem
      secondaries: []
    write_file:
      primary: dcc-mcp-skills-filesystem
      secondaries: []
  ```
- Add tests verifying no duplicate schemas exist across packages

### 4. Schema Compatibility

When the primary owner updates schemas:

- Secondary wrappers MUST either adopt the new schema or pin to a compatible version
- Breaking changes MUST be coordinated across all secondaries
- Changelog entries MUST mention all affected packages

## Enforcement

### Static Checks

Add a CI check that:

1. Scans all bundled skill packages for operation names
2. Reports operations implemented in multiple packages without `SKILL_OWNERSHIP.yml` marking one as primary
3. Validates that secondary packages do not redefine schemas already defined by the primary

### Code Review

Reviewers MUST:

- Flag duplicate operation implementations
- Request `SKILL_OWNERSHIP.yml` for multi-package PRs
- Verify secondary wrappers do not diverge in behavior

## Examples

### ✅ Compliant

**Single primary owner with thin secondary wrapper:**

```python
# Primary: dcc-mcp-skills-filesystem/skills/file-ops.py
def read_file(path: str, encoding: str = "utf-8") -> str:
    """Canonical file reader."""
    ...

# Secondary: dcc-mcp-skills-utils/skills/file_helpers.py
def read_file(path: str, **kwargs):
    """Thin wrapper delegating to primary."""
    from dcc_mcp_skills_filesystem.skills.file_ops import read_file as _primary
    return _primary(path, **kwargs)
```

### ❌ Non-Compliant

**Duplicate schema with divergent behavior:**

```python
# Package A
def read_file(path: str) -> dict:
    """Returns {"content": ...}."""
    ...

# Package B (no ownership declared)
def read_file(path: str) -> str:
    """Returns raw string, incompatible with Package A."""
    ...
```

## Migration Guide

Existing repositories with violations SHOULD:

1. Audit all skill packages for overlapping operations
2. Designate primary owners
3. Refactor secondaries to wrap or link
4. Add `SKILL_OWNERSHIP.yml`
5. Update tests

## References

- Related: [ADR-001: Skill Package Boundaries](./adr/001-skill-package-boundaries.md)
- See also: [Skills System Design](./guide/skills-system.md)
