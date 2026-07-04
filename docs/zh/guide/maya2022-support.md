# Maya 2022 支持策略

> **生效日期**：2026-07-04  
> **到期日期**：2026-12-31（可根据工作室需求延长）

## 承诺

dcc-mcp-core **将持续支持 Maya 2022 至 2026 年底**。Maya 2022 内嵌 Python 3.7 解释器，这对我们的代码库施加了以下约束。

## Maya 2022 / Python 3.7 上可用的部分

### 纯 Python 模块

所有 `dcc_mcp_core` 纯 Python 模块**语法兼容 Python 3.7**。包括：

- Skill 脚本 (`python/dcc_mcp_core/skills/`)
- Host 适配器协议 (`python/dcc_mcp_core/host/`)
- 工具注册与分发 (`python/dcc_mcp_core/_server/`)
- Schema 推导 (`python/dcc_mcp_core/schema.py`)
- 安装生命周期辅助函数

类型注解使用 `typing.Optional`、`typing.Dict`、`typing.List` 等（非 PEP 604 `|` 语法或内置泛型），确保模块在 Python 3.7 上可正常导入。

### 二进制 server wheel

`dcc-mcp-server` 配套包发布**纯 Rust 二进制**（无 Python ABI），可在任何主机上运行。Maya 2022 可以 `pip install dcc-mcp-server` 并通过 `DccServerBase` 启动 gateway daemon。

## Python 3.7 上不可用的部分

### Rust 扩展 (`_core`)

`dcc_mcp_core._core` 原生扩展使用 **PyO3 0.29** 构建，目标为 **abi3-py38**。它需要 **Python 3.8+**，无法在 Python 3.7 上加载。

这意味着：
- `from dcc_mcp_core import ToolRegistry` 在 3.7 上可用（纯 Python）
- `from dcc_mcp_core._core import ...` 在 3.7 上失败（原生扩展）

### 语义嵌入

`dcc-mcp-core-semantic` 配套 wheel 需要 **Python 3.8+**（PyO3 0.29 + ONNX Runtime）。

## Maya 2022 架构

```
┌─────────────────────────────────────────────────┐
│ Maya 2022 (Python 3.7)                          │
│                                                 │
│  ┌───────────────────────────────────────────┐  │
│  │ dcc-mcp-core (纯 Python)                  │  │
│  │  - HostAdapter、SkillCatalog、schema.py   │  │
│  │  - 可正常导入，不使用 Rust 扩展            │  │
│  └───────────────────────────────────────────┘  │
│                                                 │
│  ┌───────────────────────────────────────────┐  │
│  │ dcc-mcp-server (二进制 wheel)             │  │
│  │  - dcc-mcp-server.exe / dcc-mcp-cli       │  │
│  │  - 纯 Rust，无 Python ABI                 │  │
│  └───────────────────────────────────────────┘  │
│                                                 │
│  ┌───────────────────────────────────────────┐  │
│  │ dcc-mcp-maya 适配器                       │  │
│  │  - Maya 专用命令 + UI                     │  │
│  │  - 通过 HTTP 与 gateway 通信              │  │
│  └───────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
```

Gateway daemon（二进制，无 Python 依赖）通过 Maya 适配器连接到 Maya 的嵌入式 Python，适配器使用 `dcc-mcp-core` 纯 Python 模块进行协议定义和工具分发。

## CI 保障

| 检查项 | 频率 | 验证内容 |
|---|---|---|
| `python-matrix-full` | 每周 | 完整 Python 3.8–3.14 矩阵 |
| `python-test` | 每个 PR | Python 3.8、3.10、3.13、3.14 |
| `check_py37_syntax` | 每个 PR | 纯 Python 语法兼容 Python 3.7 |

`check_py37_syntax.py` 脚本 (`scripts/check_py37_syntax.py`) 使用 Python 3.7 对所有 `python/dcc_mcp_core/` 模块运行 `python -m py_compile`，确保纯 Python 层不会引入 Python 3.8+ 语法。

## 编写 Python 3.7 兼容代码

修改纯 Python 模块时：

- **应使用** `typing.Optional[X]` 而非 `X | None`
- **应使用** `typing.Dict[K, V]` 而非 `dict[K, V]`
- **应使用** `typing.List[X]` 而非 `list[X]`
- **应使用** `typing.Tuple[X, ...]` 而非 `tuple[X, ...]`
- **应使用** `from __future__ import annotations` 处理前向引用
- **不应使用** walrus 操作符 (`:=`)
- **不应使用** `f"{expr=}"` 调试格式字符串
- **不应使用** 仅位置参数 (`/`)

## 终止计划

- **2026-12-31**：Maya 2022 支持基线到期
- **2026-10**：社区调查 — 延长还是弃用？
- **2027-01**：如决定弃用，移除 Python 3.7 语法检查和兼容代码

## 参考

- [PyO3 0.29 更新日志](https://github.com/PyO3/pyo3/releases/tag/v0.29.0) — 移除了 Python 3.7 支持
- [Maya 2022 Python API](https://help.autodesk.com/view/MAYAUL/2022/ENU/?guid=Maya_SDK_py_ref_html)
- [ABI3 wheel 规范](https://pyo3.rs/v0.23.0/building-and-distribution#py_limited_apiabi3)
