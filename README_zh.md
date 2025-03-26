# DCC-MCP-Core

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core)](https://pypi.org/project/dcc-mcp-core/)
[![Python](https://img.shields.io/pypi/pyversions/dcc-mcp-core)](https://www.python.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://static.pepy.tech/badge/dcc-mcp-core)](https://pepy.tech/project/dcc-mcp-core)
[![Coverage](https://img.shields.io/codecov/c/github/loonghao/dcc-mcp-core)](https://codecov.io/gh/loonghao/dcc-mcp-core)
[![Tests](https://img.shields.io/github/actions/workflow/status/loonghao/dcc-mcp-core/tests.yml?branch=main&label=Tests)](https://github.com/loonghao/dcc-mcp-core/actions)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](http://makeapullrequest.com)
[![Latest Version](https://img.shields.io/github/v/tag/loonghao/dcc-mcp-core?label=Latest%20Version)](https://github.com/loonghao/dcc-mcp-core/releases)

[English](README.md) | [中文文档](README_zh.md)

DCC 模型上下文协议（Model Context Protocol，MCP）生态系统的基础库。它提供了在所有其他 DCC-MCP 包中使用的通用工具、基类和共享功能。

> **注意**：本项目处于早期开发阶段。API 可能会随时变化，我们不会提前发出通知。

## 设计理念与工作流程

DCC-MCP-Core 是一个为数字内容创建(DCC)应用程序设计的动作管理系统，旨在提供一个统一的接口，使 AI 能够与各种 DCC 软件（如 Maya、Blender、Houdini 等）进行交互。

### 核心工作流程

1. **MCP 服务器**：作为中央协调器，接收来自 AI 的请求
2. **DCC-MCP**：连接 MCP 服务器和具体的 DCC 软件
3. **动作发现与加载**：DCC-MCP-Core 负责发现、加载和管理动作
4. **结构化信息返回**：以 AI 友好的结构化格式将动作信息返回给 MCP 服务器
5. **函数调用与结果返回**：MCP 服务器调用相应的动作函数，并将结果返回给 AI

```mermaid
graph LR
    AI[AI 助手] -->|"1. 发送请求"| MCP[MCP 服务器]
    MCP -->|"2. 转发请求"| DCCMCP[DCC-MCP]
    DCCMCP -->|"3. 发现与加载"| Actions[DCC 动作]
    Actions -->|"4. 返回信息"| DCCMCP
    DCCMCP -->|"5. 结构化数据"| MCP
    MCP -->|"6. 调用函数"| DCCMCP
    DCCMCP -->|"7. 执行"| DCC[DCC 软件]
    DCC -->|"8. 操作结果"| DCCMCP
    DCCMCP -->|"9. 结构化结果"| MCP
    MCP -->|"10. 返回结果"| AI

    style AI fill:#f9d,stroke:#333,stroke-width:4px
    style MCP fill:#bbf,stroke:#333,stroke-width:4px
    style DCCMCP fill:#bbf,stroke:#333,stroke-width:4px
    style DCC fill:#bfb,stroke:#333,stroke-width:4px
    style Actions fill:#fbb,stroke:#333,stroke-width:4px
```

### 动作设计

动作采用简单直观的设计，使开发者能够轻松创建新的 DCC 功能：

- **元数据声明**：通过简单的变量定义动作的基本信息
- **函数定义**：实现特定的 DCC 操作功能
- **上下文传递**：通过上下文参数访问 DCC 软件的远程接口
- **结构化返回**：所有函数返回标准化的结构化数据

### 远程调用架构

DCC-MCP-Core 使用 RPyC 实现远程过程调用，允许在不同进程甚至不同机器上执行 DCC 操作：

- **上下文对象**：包含远程 DCC 客户端和命令接口
- **透明访问**：动作代码可以像访问本地 API 一样访问远程 DCC API
- **错误处理**：统一的错误处理机制确保稳定运行

## 包结构

DCC-MCP-Core 组织为几个子包：

- **actions**：动作管理和执行
  - `base.py`：基础 Action 类定义
  - `manager.py`：用于动作发现和执行的 ActionManager
  - `registry.py`：用于注册和检索动作的 ActionRegistry
  - `middleware.py`：用于横切关注点的中间件
  - `events.py`：用于动作通信的事件系统

- **models**：MCP 生态系统的数据模型
  - `action_result.py`：动作的结构化结果模型

- **utils**：实用函数和辅助工具
  - `module_loader.py`：模块加载工具
  - `filesystem.py`：文件系统操作
  - `decorators.py`：用于错误处理的函数装饰器
  - `platform.py`：平台特定工具

## 功能特性

- 基于类的 Action 设计，使用 Pydantic 模型
- 参数验证和类型检查
- 带有上下文和提示的结构化结果格式
- 动态动作发现和加载
- 用于横切关注点的中间件支持
- 用于动作通信的事件系统
- 异步动作执行
- 全面的错误处理

## 安装

```bash
# 从 PyPI 安装
pip install dcc-mcp-core

# 或从源代码安装
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

## 开发环境设置

```bash
# 克隆仓库
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# 创建并激活虚拟环境
python -m venv venv
source venv/bin/activate  # Windows 系统: venv\Scripts\activate

# 安装开发依赖
pip install -e .
pip install pytest pytest-cov pytest-mock pyfakefs

# 安装开发工具
pip install uvx nox ruff isort pre-commit
```

## 运行测试

```bash
# 运行测试并生成覆盖率报告
uvx nox -s pytest

# 运行特定测试
uvx nox -s pytest -- tests/test_action_manager.py -v

# 运行代码风格检查
uvx nox -s lint-fix
```

## 示例使用

### 发现和加载动作

```python
from dcc_mcp_core.actions.manager import ActionManager

# 创建一个 Maya 的动作管理器（不从环境变量加载路径）
manager = ActionManager('maya', load_env_paths=False)

# 注册动作路径
manager.register_action_path('/path/to/actions')

# 刷新动作（发现并加载）
manager.refresh_actions()

# 获取所有已注册动作的信息
actions_info = manager.get_actions_info()

# 打印可用动作的信息
for action_name, action_info in actions_info.items():
    print(f"动作: {action_name}")
    print(f"  描述: {action_info['description']}")
    print(f"  标签: {', '.join(action_info['tags'])}")

# 调用动作并传递参数
result = manager.call_action(
    'create_sphere',
    radius=2.0,
    position=[0, 1, 0],
    name='my_sphere'
)

# 访问结果
if result.success:
    print(f"成功: {result.message}")
    print(f"创建的对象: {result.context.get('object_name')}")
    if result.prompt:
        print(f"下一步建议: {result.prompt}")
else:
    print(f"错误: {result.error}")
```

### 创建自定义动作

```python
# my_maya_action.py
from dcc_mcp_core.actions.base import Action
from pydantic import Field, field_validator

class CreateSphereAction(Action):
    # 动作元数据
    name = "create_sphere"
    description = "在 Maya 中创建一个球体"
    tags = ["几何体", "创建"]
    dcc = "maya"
    order = 0

    # 带验证的输入参数模型
    class InputModel(Action.InputModel):
        radius: float = Field(1.0, description="球体的半径")
        position: list[float] = Field([0, 0, 0], description="球体的位置")
        name: str = Field(None, description="球体的名称")

        # 参数验证
        @field_validator('radius')
        def validate_radius(cls, v):
            if v <= 0:
                raise ValueError("半径必须为正数")
            return v

    # 输出数据模型
    class OutputModel(Action.OutputModel):
        object_name: str = Field(description="创建的对象名称")
        position: list[float] = Field(description="对象的最终位置")

    def _execute(self) -> None:
        # 访问经过验证的输入参数
        radius = self.input.radius
        position = self.input.position
        name = self.input.name or f"sphere_{radius}"

        # 访问 DCC 上下文（例如，Maya cmds）
        cmds = self.context.get("cmds")

        try:
            # 执行 DCC 特定的操作
            sphere = cmds.polySphere(r=radius, n=name)[0]
            cmds.move(*position, sphere)

            # 设置结构化输出
            self.output = self.OutputModel(
                object_name=sphere,
                position=position,
                prompt="现在您可以修改球体的属性或添加材质"
            )
        except Exception as e:
            # 异常将被 Action.process 方法捕获
            # 并转换为适当的 ActionResultModel
            raise Exception(f"创建球体失败: {str(e)}") from e
```

## 贡献

欢迎贡献！请随时提交 Pull Request。

## 许可证

本项目采用 MIT 许可证 - 详情请参阅 LICENSE 文件。
