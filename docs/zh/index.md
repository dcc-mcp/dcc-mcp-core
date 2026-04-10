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
    details: 通过 PyO3 将性能关键模块使用 Rust 实现。线程安全、基于 parking_lot 的数据结构，零 Python 运行时依赖。
  - icon: 🎯
    title: Action 注册表
    details: 线程安全的 Action 注册与查找。为每个 DCC 操作存储元数据、JSON Schema 和源文件路径。
  - icon: 🔌
    title: Skills-First 架构
    details: 一行 create_skill_manager("maya") 即可自动发现脚本、注册 MCP 工具并启动 HTTP 服务器。零样板代码。
  - icon: 🌐
    title: MCP HTTP 服务器
    details: 内置 Streamable HTTP 服务器（2025-03-26 规范）。AI 客户端（Claude Desktop 等）直接通过 HTTP 连接。后台运行，不阻塞 DCC 主线程。
  - icon: ⚡
    title: 事件总线
    details: 发布/订阅事件系统，用于解耦的 Action 生命周期通信。支持异常安全和线程安全。
  - icon: 🔄
    title: 传输层
    details: 连接池、基于文件的服务发现、带自动重连的会话管理，用于 DCC 通信。
  - icon: 🛡️
    title: 沙箱与安全
    details: API 白名单、输入验证和内存审计日志，确保 AI 动作执行安全可控。
  - icon: 📊
    title: 遥测
    details: 兼容 OpenTelemetry 的链路追踪、每个 Action 的性能指标和执行记录器，适用于生产环境可观测性。
  - icon: 🎬
    title: 进程管理
    details: 跨平台 DCC 进程启动、健康监控和崩溃恢复，支持可配置的恢复策略。
---
