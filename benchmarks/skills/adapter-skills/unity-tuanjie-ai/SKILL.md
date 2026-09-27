---
name: unity-tuanjie-ai
description: >-
  Domain skill — Reuse AI asset generators installed in Tuanjie Editor through
  their native Codely CustomTool contract. Use for Tuanjie image, sprite, 3D,
  material, audio, video, terrain, and session-asset tasks. Not for other Unity
  project or scene operations.
license: MIT
compatibility: "Tuanjie with cn.tuanjie.codely.bridge and a native CustomTool provider; dcc-mcp-core 0.19.90+"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: unity
    layer: domain
    version: "0.13.0"  # x-release-please-version
    search-hint: "Tuanjie AI generate image sprite 3D material audio video terrain"
    tags: "unity,tuanjie,ai,assets,generation,game-development"
    tools: tools.yaml
    depends: "dcc-diagnostics"
---

# Tuanjie AI

Call `inspect_tuanjie_ai` first and execute only a tool name returned by that fresh result. Read its
`tool_descriptions` entry for provider-owned parameter, default, and recovery guidance; a null entry
means that a manually registered tool did not publish a native description. The adapter delegates
to the installed Tuanjie package; it does not copy its HTTP client, credentials, credit rules,
downloads, or recovery logic. Tuanjie/Codely sign-in and sufficient credits remain host
prerequisites. The official `cn.tuanjie.ai.generators` package is optional: other plugins can extend
this Skill by registering valid tools with the bridge's native `CustomTool` contract.

Generation can spend credits and import or replace project assets. Pass the current agent session
identifier when the native tool supports `session_id`, then poll with the matching native status tool.
Do not retry a timed-out submission: inspect native task/session state first. Verify generated assets
and their usage terms before shipping them.
