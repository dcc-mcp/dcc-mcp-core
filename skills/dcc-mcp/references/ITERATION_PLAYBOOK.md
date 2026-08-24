# Iteration Playbook

Use the cheapest durable representation that matches the repetition.

## Same script, new values

Materialize one typed `def main(**params)` with `reuse=true` and a stable
`reuse_key`. Record `file_path`, `sha256`, and `parameters_schema`; later calls
send that same path plus changed `params`. Never resend unchanged source.

When a materialize call is repeated, require `reused=true`. When execution
accepts `sha256`, repeat it as an integrity assertion. Source changes create a
new reviewed materialization; parameter changes do not.

## Same multi-step sequence twice

Promote it to a reviewed WorkflowSpec and call `workflows_run`. Automatic
result reuse skips unchanged tool calls. Use explicit `idempotency_key` values
when the workflow needs a named scope, and recover an interrupted persisted run
with `workflows_resume`.

## Same successful procedure across three or more tasks

Compile the reviewed recording/session history or submit bounded evidence to
`review_skill_improvement`. Prefer updating the owning Skill over creating a
duplicate.

A timeout, transport loss, or missing response is not permission to
rematerialize or replay a mutation. Query the existing job or workflow first.
Keep the same caller-owned session and idempotency scope while recovering it.
