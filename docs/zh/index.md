---
layout: home
description: 通过 Skills over MCP 和 MCP 2026-07-28 发现 Agent Skills，使用 dccmcp 操作已打开的 DCC 应用。

hero:
  name: DCC-MCP-Core
  text: 面向数字内容创作的 Skills over MCP
  tagline: 通过官方 MCP 扩展和 MCP 2026-07-28 发现 Agent Skills。使用 dccmcp 在 Maya、Blender、Houdini、Photoshop 等应用中查找和调用工具。
  image:
    src: /logo.svg
    alt: DCC-MCP-Core
  actions:
    - theme: brand
      text: 快速开始
      link: /zh/guide/getting-started
    - theme: alt
      text: Skills over MCP
      link: /zh/guide/mcp-skills-integration
    - theme: alt
      text: 安装 dcc-mcp Skill
      link: https://clawhub.ai/loonghao/skills/dcc-mcp
    - theme: alt
      text: GitHub
      link: https://github.com/dcc-mcp/dcc-mcp-core

features:
  - icon: 🤖
    title: 通过 MCP 发现 Agent Skills
    details: 连接声明 Skills 扩展的 DCC 实例端点，按需读取 SKILL.md 和配套文件。
  - icon: 🎯
    title: 一个 CLI 操作多个 DCC 会话
    details: 加载公开 dcc-mcp Skill 后，用 dccmcp 通过网关查找工具、检查参数和发起调用。已有 CLI 和 REST 集成可以继续使用。
  - icon: 🔌
    title: 零代码 Skill 注册
    details: 编写 SKILL.md + scripts/ 即获得 MCP 工具。无需 Python 胶水代码。支持 Python、MEL、MaxScript、Bash、PowerShell 等。
  - icon: 🏆
    title: 版本感知网关选举
    details: 多 DCC 实例竞争网关角色，新版本 adapter 可干净接管，已有实例仍可被发现和路由。
  - icon: 🦀
    title: Rust 驱动核心
    details: 零第三方 Python 库依赖。通过 dcc-mcp-server 提供打包 gateway daemon 二进制。通过 Named Pipe 和 Unix Socket 实现零拷贝 IPC。rmp-serde 序列化。LZ4 共享内存。毫秒级工具调用。
  - icon: 🔒
    title: SkillPolicy 与作用域
    details: allow_implicit_invocation 控制 AI 是否可直接调用 Skill。products 按 DCC 过滤可见性。信任层级（repo < user < system < admin）。
  - icon: 📦
    title: 实例追踪
    details: 每个 DCC 注册 pid、display_name、当前场景和打开的文档。AI 智能体按文档或显示名路由到正确实例。
  - icon: 🛡️
    title: 沙箱与审计日志
    details: 基于策略的访问控制与内存审计日志。定义 AI 在每个 DCC 类型下能做什么，不能做什么。
  - icon: 🌐
    title: MCP Streamable HTTP + REST
    details: 内置 MCP server、REST /v1/* 动态能力面、Admin UI、审计日志、trace 与 gateway diagnostics。
  - icon: 📊
    title: 结构化结果
    details: 每个工具返回（success, message, context, next_steps）。AI 智能体能清晰推理执行结果。无需脆弱的文本解析。
---
