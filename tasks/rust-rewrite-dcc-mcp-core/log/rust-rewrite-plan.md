# dcc-mcp-core Rust 重写实施计划

## 目标
将 dcc-mcp-core 从纯 Python + Pydantic 架构重写为 Rust + PyO3 + maturin 架构，实现：
- **零 Python 依赖**（对 Python 3.8+）
- **Python 3.7-3.13 支持**（abi3-py38 + 单独 3.7 wheel）
- **保持 `dcc_mcp_core` 命名空间不变**
- 参照 auroraview 的成熟模式

## 新项目结构

```
dcc-mcp-core/
├── Cargo.toml                        # Rust workspace + 根 crate
├── pyproject.toml                    # maturin 构建配置
├── src/                              # Rust 核心源码
│   ├── lib.rs                        # PyO3 #[pymodule] 入口 → dcc_mcp_core._core
│   ├── models/
│   │   ├── mod.rs                    # 模型模块
│   │   ├── action_result.rs          # ActionResultModel #[pyclass]
│   │   └── skill_metadata.rs         # SkillMetadata #[pyclass]
│   ├── actions/
│   │   ├── mod.rs                    # Action 子系统
│   │   ├── registry.rs              # ActionRegistry #[pyclass] (线程安全 DashMap)
│   │   ├── events.rs                # EventBus #[pyclass]
│   │   └── middleware.rs            # Middleware 链
│   ├── skills/
│   │   ├── mod.rs
│   │   ├── scanner.rs               # SkillScanner #[pyclass] (并行 fs walk)
│   │   └── loader.rs                # SKILL.md YAML 解析 (serde_yaml)
│   ├── protocols/
│   │   ├── mod.rs
│   │   └── types.rs                 # ToolDefinition, ResourceDefinition 等
│   ├── utils/
│   │   ├── mod.rs
│   │   ├── filesystem.rs            # 平台目录 (dirs crate 替代 platformdirs)
│   │   ├── exceptions.rs            # MCPError 异常层次
│   │   ├── result_factory.rs        # success_result / error_result
│   │   └── type_wrappers.rs         # RPyC 类型包装
│   └── log_config.rs                # tracing 替代 loguru
├── python/dcc_mcp_core/              # Python 源码目录
│   ├── __init__.py                   # 从 _core 导入 Rust 类 + Python 封装
│   ├── actions/
│   │   ├── __init__.py
│   │   ├── base.py                  # Action 基类 (必须留 Python — 用户要继承)
│   │   ├── manager.py               # ActionManager (薄封装调用 Rust registry)
│   │   ├── function_adapter.py      # 函数适配器
│   │   └── generator.py             # Action 模板生成 (Rust 模板引擎)
│   ├── protocols/
│   │   ├── __init__.py
│   │   ├── base.py                  # Resource/Prompt ABC (必须留 Python)
│   │   ├── server.py                # MCPServerProtocol (Python Protocol)
│   │   ├── adapter.py               # MCPAdapter
│   │   └── types.py                 # 从 _core 导入 Rust 类型
│   ├── skills/
│   │   ├── __init__.py
│   │   └── script_action.py         # ScriptAction (留 Python — 需要 subprocess)
│   ├── utils/
│   │   ├── __init__.py
│   │   ├── decorators.py            # 保留 Python (需要 functools)
│   │   ├── dependency_injector.py   # 保留 Python (需要 importlib)
│   │   └── module_loader.py         # 保留 Python (需要 importlib)
│   └── template/
│       └── action.template          # Jinja2 模板 → Rust tera 模板
├── tests/                            # Python 测试
├── tests/rust/                       # Rust 集成测试
├── .github/
│   ├── workflows/
│   │   ├── build-wheels.yml         # 参照 auroraview 的 wheel 构建矩阵
│   │   ├── release.yml              # release-please + PyPI
│   │   └── ci.yml                   # Rust + Python CI
│   └── actions/
│       └── build-wheel/action.yml   # 可复用构建 action
└── justfile                          # 开发命令
```

## 技术映射（Python 依赖 → Rust 替代）

| Python 依赖 | Rust 替代 | 说明 |
|-------------|----------|------|
| `pydantic` | `serde` + PyO3 `#[pyclass]` | Rust struct 直接暴露为 Python 类 |
| `platformdirs` | `dirs` crate | 跨平台目录 |
| `loguru` | `tracing` + `tracing-subscriber` | Rust 标准日志 |
| `jinja2` | `tera` crate (或直接 format!) | 模板引擎 |
| `pyyaml` | `serde_yaml` | YAML 解析 |
| `typing_extensions` | 仅 Python 3.7 需要 | Protocol/runtime_checkable |

## Cargo.toml 关键配置

```toml
[package]
name = "dcc-mcp-core"
version = "0.11.0"
edition = "2021"

[lib]
name = "_core"
crate-type = ["cdylib", "rlib"]

[dependencies]
pyo3 = { version = "0.23", features = ["multiple-pymethods"], optional = true }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
dirs = "6.0"
dashmap = "6.1"
parking_lot = "0.12"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
thiserror = "2.0"
walkdir = "2.5"
globset = "0.4"

[features]
default = []
python-bindings = ["pyo3"]
abi3-py38 = ["pyo3/abi3-py38"]
ext-module = ["pyo3/extension-module"]
```

## pyproject.toml 配置

```toml
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "dcc-mcp-core"
version = "0.11.0"
requires-python = ">=3.7"
dependencies = [
    "typing_extensions>=4.0.0; python_version<'3.8'"
]

[tool.maturin]
python-source = "python"
module-name = "dcc_mcp_core._core"
features = ["python-bindings", "pyo3/extension-module", "abi3-py38"]
bindings = "pyo3"
```

## 实施阶段

### Phase 1: 项目骨架搭建 (Day 1)
- [ ] 创建 Cargo.toml + pyproject.toml
- [ ] 创建 src/lib.rs (#[pymodule] 入口)
- [ ] 创建 python/dcc_mcp_core/__init__.py
- [ ] 验证 `maturin develop` 可构建

### Phase 2: 核心模型迁移 (Day 1-2)
- [ ] Rust: ActionResultModel #[pyclass] (替代 Pydantic BaseModel)
- [ ] Rust: SkillMetadata #[pyclass]
- [ ] Rust: success_result / error_result / from_exception 工厂函数
- [ ] Rust: MCPError 异常层次 (#[pyclass] extend=PyException)
- [ ] Rust: ToolDefinition, ResourceDefinition 等协议类型

### Phase 3: ActionRegistry Rust 实现 (Day 2-3)
- [ ] Rust: ActionRegistry with DashMap (线程安全，无需单例)
- [ ] Rust: EventBus with crossbeam-channel
- [ ] Rust: 平台目录函数 (dirs crate)
- [ ] Python: Action 基类保留 (用户继承点)
- [ ] Python: ActionManager 薄封装

### Phase 4: Skills 系统迁移 (Day 3)
- [ ] Rust: SkillScanner (并行文件扫描 + serde_yaml)
- [ ] Rust: SKILL.md 解析器
- [ ] Python: ScriptAction 保留 (需要 subprocess)

### Phase 5: 日志 + 工具函数 (Day 3-4)
- [ ] Rust: tracing 日志系统
- [ ] Rust: type_wrappers (RPyC 类型保护)
- [ ] Python: decorators, dependency_injector, module_loader 保留

### Phase 6: CI/CD + Wheel 构建 (Day 4)
- [ ] GitHub Actions: build-wheels.yml (参照 auroraview)
- [ ] 构建矩阵: Windows/Linux/macOS × abi3 + py37
- [ ] release.yml: release-please + PyPI

### Phase 7: 测试迁移 (Day 4-5)
- [ ] 迁移现有 Python 测试
- [ ] 添加 Rust 单元测试
- [ ] 验证 3.7-3.13 兼容性

## 什么留在 Python，什么用 Rust

### 必须留在 Python 的
1. `actions/base.py` — **Action 基类**：用户需要继承并实现 `_execute()`
2. `protocols/base.py` — **Resource/Prompt ABC**：用户需要继承
3. `protocols/server.py` — **Protocol 定义**：Python 类型系统概念
4. `utils/decorators.py` — 需要 `functools.wraps`
5. `utils/dependency_injector.py` — 需要 `importlib`
6. `utils/module_loader.py` — 需要 `importlib.util`
7. `skills/script_action.py` — 需要 `subprocess`
8. `actions/generator.py` — 模板生成（可选迁移到 Rust tera）

### 用 Rust 实现的
1. `ActionResultModel` — 纯数据结构，serde 序列化
2. `SkillMetadata` — 纯数据结构
3. `ActionRegistry` — HashMap 操作，DashMap 线程安全
4. `EventBus` — 事件订阅/发布，crossbeam
5. `ToolDefinition` 等协议类型 — 纯数据结构
6. `MCPError` 异常层次 — Rust thiserror
7. 平台目录函数 — dirs crate
8. type_wrappers — 纯类型包装
9. result_factory — 工厂函数
10. SkillScanner — 文件系统扫描
11. SKILL.md 解析 — serde_yaml
12. 日志配置 — tracing

## 向后兼容保证

所有公开 API 保持不变：
```python
# 这些导入全部保持工作
from dcc_mcp_core import ActionResultModel, create_action_manager
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.protocols import MCPAdapter, ToolDefinition
from dcc_mcp_core.skills import SkillScanner, load_skill
```
