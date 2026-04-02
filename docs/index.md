---
layout: home

hero:
  name: DCC-MCP-Core
  text: AI ↔ DCC Bridge
  tagline: Rust-powered foundational library for the DCC Model Context Protocol ecosystem. Seamlessly connect AI with Maya, Blender, Houdini, and more.
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
  - icon: 🦀
    title: Rust-Powered Core
    details: Performance-critical modules in Rust via PyO3. Thread-safe, lock-free data structures with DashMap. Zero Python dependencies.
  - icon: 🎯
    title: Action Registry
    details: Thread-safe action registration and lookup. Store metadata, JSON schemas, and source paths for each DCC operation.
  - icon: 🔌
    title: Skills System
    details: Register any script (Python, MEL, MaxScript, BAT, Shell) as MCP tools with zero code via SKILL.md.
  - icon: ⚡
    title: Event Bus
    details: Publish/subscribe event system for decoupled action lifecycle communication. Panic-safe and thread-safe.
  - icon: 🌐
    title: MCP Protocol Types
    details: Type-safe definitions for MCP Tools, Resources, Prompts, and Annotations following the official specification.
  - icon: 🔄
    title: Transport Layer
    details: Connection pooling, file-based service discovery, session management with auto-reconnection for DCC communication.
---
