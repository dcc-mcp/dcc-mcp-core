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
    details: Performance-critical modules in Rust via PyO3. Thread-safe data structures with parking_lot. Zero Python runtime dependencies.
  - icon: 🎯
    title: Action Registry
    details: Thread-safe action registration and lookup. Store metadata, JSON schemas, and source paths for each DCC operation.
  - icon: 🔌
    title: Skills-First Architecture
    details: One call to create_skill_manager("maya") auto-discovers scripts via SKILL.md, registers MCP tools, and starts an HTTP server. Zero boilerplate.
  - icon: 🌐
    title: MCP HTTP Server
    details: Built-in Streamable HTTP server (2025-03-26 spec). LLM clients (Claude Desktop, etc.) connect directly over HTTP. Runs in background, never blocks DCC.
  - icon: ⚡
    title: Event Bus
    details: Publish/subscribe event system for decoupled action lifecycle communication. Panic-safe and thread-safe.
  - icon: 🔄
    title: Transport Layer
    details: Connection pooling, file-based service discovery, session management with auto-reconnection for DCC communication.
  - icon: 🛡️
    title: Sandbox & Security
    details: API whitelist, input validation, and in-memory audit logging for safe AI action execution.
  - icon: 📊
    title: Telemetry
    details: OpenTelemetry-compatible tracing, per-action metrics, and execution recorder for production observability.
  - icon: 🎬
    title: Process Management
    details: Cross-platform DCC process launch, health monitoring, and crash recovery with configurable policies.
---
