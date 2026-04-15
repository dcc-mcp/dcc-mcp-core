---
layout: home

hero:
  name: DCC-MCP-Core
  text: 面向 DCC 的 MCP + Skills
  tagline: 生产级 MCP 与零代码 Skills 系统的结合。解决上下文爆炸，将 AI 与 Maya、Blender、Houdini、Photoshop 高性能对接。
  image:
    src: /logo.svg
    alt: DCC-MCP-Core
  actions:
    - theme: brand
      text: 快速开始
      link: /zh/guide/getting-started
    - theme: alt
      text: 为什么选择 MCP + Skills？
      link: /zh/guide/what-is-dcc-mcp-core
    - theme: alt
      text: GitHub
      link: https://github.com/loonghao/dcc-mcp-core

features:
  - icon: 🎯
    title: 解决 MCP 上下文爆炸
    details: 会话隔离将每个 AI 会话绑定到单一 DCC 实例。tools/list 返回 150 个工具而非 750 个。按 DCC 类型、作用域和产品进行渐进式发现。
  - icon: 🔌
    title: 零代码 Skill 注册
    details: 编写 SKILL.md + scripts/ 即获得 MCP 工具。无需 Python 胶水代码。支持 Python、MEL、MaxScript、Bash、PowerShell 等。
  - icon: 🏆
    title: 版本感知网关选举
    details: 多 DCC 实例竞争网关角色，最新版本自动接管。无需手动故障转移——语义版本比较自动完成。
  - icon: 🦀
    title: Rust 驱动核心
    details: 零运行时 Python 依赖。通过 Named Pipe 和 Unix Socket 实现零拷贝 IPC。rmp-serde 序列化。LZ4 共享内存。毫秒级工具调用。
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
    title: MCP Streamable HTTP
    details: 内置服务器（2025-03-26 规范）。Claude Desktop、Cursor 等 MCP 客户端直接通过 HTTP 连接。后台线程，从不阻塞 DCC。
  - icon: 📊
    title: 结构化结果
    details: 每个工具返回（success, message, context, next_steps）。AI 智能体能清晰推理执行结果。无需脆弱的文本解析。
---
