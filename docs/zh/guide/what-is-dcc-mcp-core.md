# 什么是 DCC-MCP-Core？

DCC-MCP-Core 是一个为数字内容创建（DCC）应用程序设计的**动作管理系统**，提供统一的接口，使 AI 能够与各种 DCC 软件（如 Maya、Blender、Houdini 等）进行交互。

## 核心工作流程

```mermaid
flowchart LR
    AI([AI 助手]):::aiNode
    MCP{{MCP 服务器}}:::serverNode
    DCCMCP{{DCC-MCP}}:::serverNode
    Actions[(DCC 动作)]:::actionsNode
    DCC[/DCC 软件/]:::dccNode

    AI -->|1. 发送请求| MCP
    MCP -->|2. 转发请求| DCCMCP
    DCCMCP -->|3. 发现与加载| Actions
    Actions -->|4. 返回信息| DCCMCP
    DCCMCP -->|5. 结构化数据| MCP
    MCP -->|6. 调用函数| DCCMCP
    DCCMCP -->|7. 执行操作| DCC
    DCC -->|8. 操作结果| DCCMCP
    DCCMCP -->|9. 结构化结果| MCP
    MCP -->|10. 返回结果| AI

    classDef aiNode fill:#f9d,stroke:#f06,stroke-width:2px,color:#333
    classDef serverNode fill:#bbf,stroke:#66f,stroke-width:2px,color:#333
    classDef dccNode fill:#bfb,stroke:#6b6,stroke-width:2px,color:#333
    classDef actionsNode fill:#fbb,stroke:#f66,stroke-width:2px,color:#333
```

## 核心特性

- **基于类的 Action 设计** — 使用 Pydantic 模型，提供强类型检查和输入验证
- **ActionManager** — 动作生命周期协调器，负责发现、加载和执行
- **中间件系统** — 责任链模式，支持日志记录、性能监控等
- **事件系统** — 发布/订阅 EventBus，用于动作生命周期事件
- **Skills 技能包** — 零代码将脚本注册为 MCP 工具
- **MCP 协议层** — 完整的 Tools、Resources 和 Prompts 抽象
- **零依赖** — Python 3.8+ 无第三方 Python 依赖
- **Rust 核心** — 通过 PyO3 提供 Rust 编写的高性能模块

## 项目结构

```
dcc_mcp_core/
├── __init__.py              # 公共 API 导出
├── models.py                # ActionResultModel, SkillMetadata
├── actions/
│   ├── base.py              # Action 基类
│   ├── manager.py           # ActionManager
│   ├── registry.py          # ActionRegistry（单例）
│   ├── middleware.py         # 中间件系统
│   ├── events.py            # EventBus
│   ├── function_adapter.py  # Action 转函数适配器
│   └── generator.py         # Action 模板生成
├── skills/
│   ├── scanner.py           # SkillScanner
│   ├── loader.py            # SKILL.md 解析器
│   └── script_action.py     # ScriptAction 工厂
├── protocols/
│   ├── types.py             # MCP 类型定义
│   ├── base.py              # Resource, Prompt 抽象基类
│   ├── server.py            # MCPServerProtocol
│   └── adapter.py           # MCPAdapter
└── utils/
    ├── filesystem.py         # 平台目录、环境路径
    ├── module_loader.py      # 动态模块加载
    ├── decorators.py         # error_handler, with_context
    ├── dependency_injector.py
    ├── template.py           # Jinja2 渲染
    ├── type_wrappers.py      # RPyC 安全类型包装器
    └── result_factory.py     # 工厂函数
```

## 相关项目

- [dcc-mcp-rpyc](https://github.com/loonghao/dcc-mcp-rpyc) — 用于远程 DCC 操作的 RPyC 桥接
- [dcc-mcp-maya](https://github.com/loonghao/dcc-mcp-maya) — Maya MCP 服务器实现
