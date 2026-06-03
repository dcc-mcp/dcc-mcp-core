---
layout: home

hero:
  name: DCC-MCP-Core
  text: Gateway-first MCP + Skills for DCC AI
  tagline: Production-grade foundation combining MCP, zero-code Skills, dynamic capability routing, and release-ready CLI/server binaries for Maya, Blender, Houdini, Photoshop, and custom studio hosts.
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
      link: https://github.com/dcc-mcp/dcc-mcp-core

features:
  - icon: 🎯
    title: Dynamic Capability Routing
    details: Agents search, describe, load skills, and call tools through one gateway instead of carrying every backend tool in context.
  - icon: 🔌
    title: Zero-Code Skill Registration
    details: Write SKILL.md + scripts/ → instant MCP tools. No Python glue code needed. Supports Python, MEL, MaxScript, Bash, PowerShell, and more.
  - icon: 🏆
    title: Version-Aware Gateway Election
    details: Multiple DCC instances compete for the gateway role. Newer adapters can take over cleanly while existing instances stay discoverable.
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
    title: MCP Streamable HTTP + REST
    details: Built-in MCP server, REST /v1/* dynamic-capability surface, Admin UI, audit logs, traces, and gateway diagnostics.
  - icon: 📊
    title: Structured Results
    details: Every tool returns (success, message, context, next_steps). AI agents reason clearly about outcomes. No fragile text parsing.
---
