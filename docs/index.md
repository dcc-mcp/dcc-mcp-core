---
layout: home

hero:
  name: DCC-MCP-Core
  text: MCP + Skills for DCC AI
  tagline: Production-grade foundation combining Model Context Protocol (MCP) and a zero-code Skills system. Connect AI to Maya, Blender, Houdini, Photoshop — without context explosion.
  image:
    src: /logo.svg
    alt: DCC-MCP-Core
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: Why MCP + Skills?
      link: /guide/what-is-dcc-mcp-core
    - theme: alt
      text: GitHub
      link: https://github.com/loonghao/dcc-mcp-core

features:
  - icon: 🎯
    title: Solves MCP Context Explosion
    details: Session isolation pins each AI session to one DCC instance. tools/list returns 150 tools instead of 750. Progressive discovery by DCC type, scope, and product.
  - icon: 🔌
    title: Zero-Code Skill Registration
    details: Write SKILL.md + scripts/ → instant MCP tools. No Python glue code needed. Supports Python, MEL, MaxScript, Bash, PowerShell, and more.
  - icon: 🏆
    title: Version-Aware Gateway Election
    details: Multiple DCC instances compete for gateway role. Newest version automatically takes over. No manual failover — just semantic version comparison.
  - icon: 🦀
    title: Rust-Powered Core
    details: Zero runtime Python dependencies. Zero-copy IPC via Named Pipes and Unix Sockets. rmp-serde serialization. LZ4 shared memory. Sub-millisecond tool calls.
  - icon: 🔒
    title: SkillPolicy & Scope
    details: allow_implicit_invocation controls whether AI can call a skill without explicit load_skill. products filter visibility by DCC. Trust levels (repo < user < system < admin).
  - icon: 📦
    title: Instance Tracking
    details: Every DCC registers pid, display_name, active scene, and open documents. AI agents route to the right instance by document or display name.
  - icon: 🛡️
    title: Sandbox & Audit Log
    details: Policy-based access control with in-memory audit logging. Define what AI can and cannot do per DCC type.
  - icon: 🌐
    title: MCP Streamable HTTP
    details: Built-in server (2025-03-26 spec). Claude Desktop, Cursor, and other MCP clients connect directly over HTTP. Background thread, never blocks DCC.
  - icon: 📊
    title: Structured Results
    details: Every tool returns (success, message, context, next_steps). AI agents reason clearly about outcomes. No fragile text parsing.
---
