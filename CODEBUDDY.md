# CODEBUDDY.md

> **This project uses the [AGENTS.md](AGENTS.md) standard as the single source of truth.**
> Tencent CodeBuddy Code — read [`AGENTS.md`](AGENTS.md) first.
> This file exists only so CodeBuddy tooling that looks for `CODEBUDDY.md` by name can find its way here.

## Entry Points (read in this order)

1. [`AGENTS.md`](AGENTS.md) — navigation map, response language, PR rules, top traps, decision tables
2. [`AI_AGENT_GUIDE.md`](AI_AGENT_GUIDE.md) — skills-first workflow tutorial for AI agents
3. [`llms.txt`](llms.txt) — compressed API index (use when you need to *call* APIs)
4. [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md) — detailed rules, traps, code examples

## CodeBuddy-Specific Notes

- CodeBuddy's built-in task management tools (`TaskCreate`, `TaskUpdate`, `TaskList`) are the preferred way to track multi-step work in this repo — use them instead of ad-hoc TODOs in chat.
- CodeBuddy's `Agent` tool (with `subagent_type=Explore`) is preferred over direct `grep` / `find` for open-ended codebase exploration; it keeps the main context small.
- Everything else is canonicalised in `AGENTS.md`. Keep this file minimal — add CodeBuddy-specific guidance here **only** when it genuinely differs from the common agent guidance.
