# typed-schema-demo

Companion example for issue #242 — shows a typed handler whose
`inputSchema` and `outputSchema` are derived entirely from Python type
annotations, with no `pydantic` / `jsonschema` / `attrs` dependency.

See `SKILL.md` for the skill metadata and `scripts/demo.py` for the
handler + schema derivation.

To print the derived schemas:

```bash
python -m examples.skills.typed-schema-demo.scripts.demo
```

The shape matches what `pydantic`'s `model_json_schema()` would emit
(same `title`, `$defs`, `$ref`, `anyOf` conventions) so adopters who
later switch to pydantic will not need to migrate agents or cached
schemas.
