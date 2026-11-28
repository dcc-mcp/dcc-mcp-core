---
layout: home
description: Discover Agent Skills with Skills over MCP and MCP 2026-07-28. Use dccmcp to run tools in live DCC sessions.

hero:
  name: DCC-MCP-Core
  text: Skills over MCP for DCC
  tagline: Discover Agent Skills through the official MCP extension and MCP 2026-07-28. Use dccmcp to find and call tools in Maya, Blender, Houdini, Photoshop, and other studio hosts.
  image:
    src: /logo.svg
    alt: DCC-MCP-Core
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: Skills over MCP
      link: /guide/mcp-skills-integration
    - theme: alt
      text: Install dcc-mcp Skill
      link: https://clawhub.ai/loonghao/skills/dcc-mcp
    - theme: alt
      text: GitHub
      link: https://github.com/dcc-mcp/dcc-mcp-core

features:
  - icon: 🤖
    title: Discover Agent Skills over MCP
    details: Read SKILL.md and supporting files on demand from a DCC endpoint that advertises the Skills extension.
  - icon: 🎯
    title: One CLI for Your DCC Sessions
    details: Use the public dcc-mcp Skill and dccmcp to search, inspect, and call typed tools through the gateway. Existing CLI and REST integrations continue to work.
  - icon: 🔌
    title: Zero-Code Skill Registration
    details: Write SKILL.md + scripts/ → instant MCP tools. No Python glue code needed. Supports Python, MEL, MaxScript, Bash, PowerShell, and more.
  - icon: 🏆
    title: Version-Aware Gateway Election
    details: Multiple DCC instances compete for the gateway role. Newer adapters can take over cleanly while existing instances stay discoverable.
  - icon: 🦀
    title: Rust-Powered Core
    details: Zero third-party Python library dependencies. Packaged gateway daemon binary via dcc-mcp-server. Zero-copy IPC via Named Pipes and Unix Sockets. rmp-serde serialization. LZ4 shared memory. Sub-millisecond tool calls.
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
