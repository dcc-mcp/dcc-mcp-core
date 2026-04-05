---
layout: home

hero:
  name: DCC-MCP-Core
  text: AI ↔ DCC 桥梁
  tagline: 基于 Rust 的 DCC 模型上下文协议生态系统基础库。无缝连接 AI 与 Maya、Blender、Houdini 等 DCC 软件。
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
  - icon: 🦀
    title: Rust 驱动核心
    details: 通过 PyO3 将性能关键模块使用 Rust 实现。线程安全、基于 DashMap 的无锁数据结构，零 Python 依赖。
  - icon: 🎯
    title: Action 注册表
    details: 线程安全的 Action 注册与查找。为每个 DCC 操作存储元数据、JSON Schema 和源文件路径。
  - icon: 🔌
    title: Skills 技能包系统
    details: 通过 SKILL.md 零代码将任何脚本（Python、MEL、MaxScript、BAT、Shell）注册为 MCP 工具。
  - icon: ⚡
    title: 事件总线
    details: 发布/订阅事件系统，用于解耦的 Action 生命周期通信。支持异常安全和线程安全。
  - icon: 🌐
    title: MCP 协议类型
    details: 遵循官方规范的 MCP Tools、Resources、Prompts 和 Annotations 类型安全定义。
  - icon: 🔄
    title: 传输层
    details: 连接池、基于文件的服务发现、带自动重连的会话管理，用于 DCC 通信。
---
