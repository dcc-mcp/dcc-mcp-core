---
layout: home

hero:
  name: DCC-MCP-Core
  text: AI ↔ DCC Bridge
  tagline: Foundational library for the DCC Model Context Protocol ecosystem. Seamlessly connect AI with Maya, Blender, Houdini, and more.
  image:
    src: /logo.svg
    alt: DCC-MCP-Core
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: View on GitHub
      link: https://github.com/loonghao/dcc-mcp-core

features:
  - icon: 🎯
    title: Class-Based Actions
    details: Define operations with Pydantic models for strong typing, input validation, and structured output.
  - icon: ⚡
    title: Zero Dependencies
    details: Pure Python 3.8+ with zero third-party dependencies. Rust-powered core via PyO3 for maximum performance.
  - icon: 🔌
    title: Skills System
    details: Register any script (Python, MEL, MaxScript, BAT, Shell) as MCP tools with zero code via SKILL.md.
  - icon: 🧩
    title: Middleware & Events
    details: Chain-of-responsibility middleware and publish/subscribe event system for extensible action processing.
  - icon: 🌐
    title: MCP Protocol Layer
    details: Full MCP Server protocol with Tools, Resources, and Prompts abstractions for AI-DCC integration.
  - icon: 🔄
    title: Async Support
    details: Both synchronous and asynchronous action execution with native async override capability.
---
