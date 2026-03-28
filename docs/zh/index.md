---
layout: home

hero:
  name: DCC-MCP-Core
  text: AI ↔ DCC 桥梁
  tagline: DCC 模型上下文协议生态系统的基础库。无缝连接 AI 与 Maya、Blender、Houdini 等 DCC 软件。
  image:
    src: /logo.svg
    alt: DCC-MCP-Core
  actions:
    - theme: brand
      text: 快速开始
      link: /zh/guide/getting-started
    - theme: alt
      text: 在 GitHub 上查看
      link: https://github.com/loonghao/dcc-mcp-core

features:
  - icon: 🎯
    title: 基于类的 Action 设计
    details: 使用 Pydantic 模型定义操作，提供强类型检查、输入验证和结构化输出。
  - icon: ⚡
    title: 零依赖
    details: 纯 Python 3.8+ 实现，零第三方依赖。通过 PyO3 提供 Rust 驱动的高性能核心。
  - icon: 🔌
    title: Skills 技能包系统
    details: 通过 SKILL.md 零代码将任何脚本（Python、MEL、MaxScript、BAT、Shell）注册为 MCP 工具。
  - icon: 🧩
    title: 中间件 & 事件系统
    details: 责任链模式的中间件和发布/订阅事件系统，提供可扩展的动作处理机制。
  - icon: 🌐
    title: MCP 协议层
    details: 完整的 MCP Server 协议实现，包含 Tools、Resources 和 Prompts 抽象层。
  - icon: 🔄
    title: 异步支持
    details: 同时支持同步和异步动作执行，并提供原生异步重写能力。
---
