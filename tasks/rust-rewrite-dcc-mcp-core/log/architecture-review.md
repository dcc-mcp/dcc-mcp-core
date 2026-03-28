# dcc-mcp-core 架构深度分析报告

## 生态全景

```
dcc-mcp-core       ← 核心公用库（本仓库）
dcc-mcp-rpyc       ← RPyC 远程调用桥接层
dcc-mcp-maya       ← Maya 具体实现
dcc-mcp-3dsmax     ← 3ds Max 具体实现
dcc-mcp-houdini    ← Houdini 具体实现
dcc-mcp-blender    ← Blender 具体实现
dcc-mcp-substance  ← Substance Painter 具体实现
dcc-mcp            ← 顶层 MCP Server 聚合入口
```

**核心卖点**: 渐进式发现和复用 Skills 的机制，让各 DCC 软件能调用 Skills 去生产。

---

## 一、严重架构缺陷（Must Fix）

### 1.1 ActionRegistry 单例设计有「隐蔽破坏」问题

**问题**: `ActionRegistry` 是全局单例，但 `ActionManager` 构造时 `registry or ActionRegistry()` 总是拿到同一个实例。多个 DCC 的 ActionManager 共享同一个 `_actions` 和 `_dcc_actions` 字典。

```python
# actions/__init__.py L22
registry = ActionRegistry()  # 全局变量暴露出来

# manager.py L101
self.registry = registry or ActionRegistry()  # 永远是同一个实例
```

**后果**:
- **跨 DCC 污染**: 如果同一进程加载 maya 和 blender 的 Actions，它们共享注册表，`get_action("create_sphere")` 可能返回错误 DCC 的 Action
- **测试脆弱**: `reset()` 需要手动调用，跨测试泄漏状态（conftest 已经有大量 workaround 证实这个问题）
- **多线程竞争**: 注册表无锁保护，`_actions` dict 在并发注册时可能损坏

**建议**:
- **方案 A（推荐）**: 放弃单例，让每个 `ActionManager` 拥有自己的 `ActionRegistry` 实例；全局注册表改为可选的 "shared registry"
- **方案 B**: 如果保留单例，需加锁 + namespace 隔离（如 `_actions` 按 `dcc_name` 彻底隔离）

### 1.2 `Action` 不是 Pydantic 模型但承载状态 — 生命周期混乱

**问题**: `Action` 是纯 Python 类（非 BaseModel），但 `InputModel` 和 `OutputModel` 是 Pydantic 模型。`self.input`、`self.output`、`self.context` 都是可变实例属性，没有类型注解。

```python
class Action:
    def __init__(self, context=None):
        self.input = None    # ← 类型是什么？什么时候变成 InputModel？
        self.output = None   # ← 子类可能忘记设置
        self.context = context or {}
```

**后果**:
- `mypy --strict` 实际上对 Action 内部几乎无效（`ignore_missing_imports = true` + `disable_error_code` 在 pyproject.toml 证实）
- `self.input` 在 `setup()` 之前是 `None`，在 `_execute()` 中直接使用，如果有人忘记 `setup()` 就会 NPE
- `self.output` 是否被设置完全依赖子类纪律，没有协议约束

**建议**:
- 为 `Action` 添加明确的类型注解: `input: Optional[InputModel]`, `output: Optional[OutputModel]`
- 考虑将 `setup()` → `process()` 合并为一个调用，避免"忘记 setup"的风险
- 或者用 `__init_subclass__` 校验子类必须定义 `_execute`

### 1.3 `process()` 和 `_execute()` 的职责分裂

**问题**: `Action.process()` 是模板方法，内部调用 `_execute()`。但 `tests/example_actions/test_action.py` 中的 `TestAction` **覆写了 `process()` 而非 `_execute()`**！这说明 API 设计给用户传达了错误信号。

```python
# test_action.py — 用户覆写了 process()！
class TestAction(Action):
    def process(self) -> ActionResultModel:  # ← 本应只覆写 _execute
        ...
```

**后果**:
- 覆写 `process()` 跳过了 `Action.process()` 中的错误处理和事件发布
- `success_result` / `from_exception` 等工厂函数在覆写场景下被跳过
- 中间件链的 `action.process()` 调用会触发用户的覆写而非标准流程

**建议**:
- 将 `process()` 标记为 `final`（Python 3.8+ `typing.final`）或在文档中强制约束
- 考虑 `_execute()` 返回 `OutputModel` 而不是 void + `self.output` 赋值模式
- 添加 `__init_subclass__` 检查确保子类不覆写 `process()`

### 1.4 Python 3.7 支持拖累整个生态

**问题**: `python = ">=3.7,<4.0"` 但依赖 `pydantic>=2.0.0`（实际不支持 3.7）。

```toml
[tool.poetry.dependencies]
python = ">=3.7,<4.0"      # ← 声称 3.7
pydantic = ">=2.0.0,<3.0.0" # ← Pydantic v2 要求 3.8+
```

**后果**:
- 声称支持但实际无法运行在 3.7
- 限制了可用的语言特性（walrus operator、TypedDict 改进等）
- `typing.Protocol` 需要 `typing_extensions` 在 3.7，但未声明依赖

**建议**: 明确 `python = ">=3.9,<4.0"`。DCC 嵌入 Python 的现状：Maya 2024+ = 3.10, Houdini 20+ = 3.10, Blender 4+ = 3.11。只有极老版本用 3.7/3.8。

---

## 二、中度设计问题（Should Fix）

### 2.1 `loguru` 作为硬依赖但又做可选导入

```python
# log_config.py L34
try:
    from loguru import logger as loguru_logger
    LOGURU_AVAILABLE = True
except ImportError:
    LOGURU_AVAILABLE = False
```

但 `pyproject.toml` 中 `loguru = ">=0.7.3,<0.8.0"` 是**硬依赖**。这段 try/except 永远不会 fail（除非安装损坏）。这违反了全局提示词中的 "badcase" 规则。

**建议**: 既然是硬依赖，就直接 `from loguru import logger` 不要 try/except。或者把 loguru 移到 extras。

### 2.2 `ActionManager.call_action()` 上的 `@error_handler` 装饰器

```python
@error_handler
def call_action(self, action_name, context=None, **kwargs) -> ActionResultModel:
    ...
```

`error_handler` 装饰器会**吞掉所有异常**并转为 `ActionResultModel`。但 `call_action` 内部已经有完整的 try/except → `ActionResultModel` 逻辑。双重包装导致：
- 异常信息可能被格式化两次
- `format_result()` 会把已经是 `ActionResultModel` 的返回值再包一层

**建议**: 去掉 `call_action` 上的 `@error_handler`，它已经自己处理了所有异常。

### 2.3 MCP 协议层与 Action 层耦合不清

`protocols/` 包定义了 `MCPServerProtocol`、`ToolDefinition` 等，但实际的 MCP SDK 集成（如 `@mcp.tool()` 装饰器注册）不在 core 中。这意味着下游每个 DCC 包都要自己：

1. 遍历 `ActionManager.registry.list_actions()`
2. 用 `MCPAdapter.action_to_tool()` 生成 `ToolDefinition`
3. 注册到 MCP Server SDK

**问题**: 这部分**重复逻辑**会出现在每个 `dcc-mcp-*` 包中。

**建议**: 在 core 中提供一个 `MCPServerBuilder` / `create_mcp_server(manager)` 工厂函数，自动从 ActionManager 构建 MCP Server 的工具列表。或者提供一个 mixin/基类供下游继承。

### 2.4 Skills 系统的 `dcc_adapter` 接口未定义

```python
# script_action.py L179
dcc_adapter = self.context.get("dcc_adapter")
if dcc_adapter and hasattr(dcc_adapter, "execute"):
    result = dcc_adapter.execute(script_content, script_type=script_type)
```

`dcc_adapter` 是一个纯鸭子类型接口，没有任何 Protocol 或 ABC 定义。下游实现者无法知道需要实现什么方法。

**建议**: 在 `protocols/` 中定义 `DCCAdapterProtocol`:
```python
@runtime_checkable
class DCCAdapterProtocol(Protocol):
    def execute(self, script_content: str, script_type: str) -> Dict[str, Any]: ...
```

### 2.5 `context` 字典是万能口袋 — 类型不安全

`self.context` 在整个系统中是 `Dict[str, Any]`，往里塞了：manager、event_bus、registry、platform、python_version、timestamp、dcc_name、cmds（Maya 用户手动放的）、dcc_adapter 等。

**后果**:
- 无法 IDE 自动补全
- 无法做静态分析
- 下游不知道该放什么

**建议**: 定义 `ActionContext` Pydantic 模型或 TypedDict:
```python
class ActionContext(TypedDict, total=False):
    dcc_name: str
    manager: ActionManager
    event_bus: EventBus
    registry: ActionRegistry
    dcc_adapter: DCCAdapterProtocol
    platform: str
    python_version: str
```

### 2.6 `function_adapter.py` 过度防御性编程

`create_function_adapter` 和 `create_function_adapters` 中有 5-6 层嵌套 try/except，每一层都返回 `ActionResultModel(success=False, ...)`。这导致真正的异常信息被不同层覆盖，调试困难。

**建议**: 简化为 1-2 层异常处理。让底层异常自然传播，只在最外层捕获并转换。

---

## 三、设计改进建议（Nice to Have）

### 3.1 `Action._execute()` 的返回值设计

当前 `_execute()` 返回 `None`，通过 `self.output = OutputModel(...)` 副作用传递输出。这不是函数式风格，容易遗忘。

**建议**: 改为:
```python
def _execute(self) -> OutputModel:
    return self.OutputModel(object_name="sphere1", position=[0,0,0])
```

`process()` 内部接收返回值而非检查 `self.output`。

### 3.2 缺少 Capability Negotiation

生态中的 DCC 包能力不同（有的支持 Resources、有的只支持 Tools），但没有统一的能力协商机制。

**建议**: 在 ActionManager 或 DCC 描述中添加 capabilities 声明:
```python
class DCCCapabilities(BaseModel):
    supports_tools: bool = True
    supports_resources: bool = False
    supports_prompts: bool = False
    supports_async: bool = False
    python_version: str = ""
    platform: str = ""
```

### 3.3 缺少版本协商 / 兼容性矩阵

`dcc-mcp-core 0.10.0` 与 `dcc-mcp-rpyc 0.x.y` 之间没有版本兼容性声明。当 core 的 Action 接口变化时，rpyc 层可能不兼容。

**建议**: 在 core 中定义 `PROTOCOL_VERSION` 常量，rpyc 和下游包在连接时校验协议版本。

### 3.4 `EventBus` 全局单例 + 事件名硬编码

```python
# events.py L157
event_bus = EventBus()  # 全局唯一
```

事件名像 `"action.before_execute.{name}"` 是字符串硬编码，拼写错误无法检测。

**建议**:
- 用 `enum` 或常量类定义事件名
- EventBus 可以非全局化，让每个 ActionManager 拥有自己的实例

### 3.5 测试中存在大量 "coverage boost" 文件

```
test_coverage_boost.py      (34.68 KB)
test_coverage_boost_phase2.py (30.38 KB)
test_coverage_phase3.py     (38.05 KB)
```

这些文件总计 ~103KB 测试代码，文件名暗示是为提高覆盖率而非验证行为而写。

**建议**: 重构为按模块组织的有意义测试，而非按"阶段"堆积。好的测试应该能描述被测行为。

### 3.6 `log_config.py` 模块导入时有副作用

```python
LOG_DIR = Path(get_log_dir())
LOG_DIR.mkdir(parents=True, exist_ok=True)  # ← 导入时创建目录
```

**建议**: 延迟到第一次调用 `get_logger()` 时创建。

---

## 四、生态级架构建议

### 4.1 核心层次应该更薄

当前 `dcc-mcp-core` 包含了太多可以拆分的模块：

| 模块 | 建议归属 |
|------|---------|
| `actions/` (base, registry, manager, middleware, events) | **core** ✓ |
| `models.py` (ActionResultModel, SkillMetadata) | **core** ✓ |
| `protocols/` (types, server, adapter) | **core** ✓ |
| `skills/` (scanner, loader, script_action) | 可拆为 `dcc-mcp-skills` 独立包 |
| `utils/type_wrappers.py` | 应移到 `dcc-mcp-rpyc` |
| `utils/pydantic_extensions.py` | 应评估是否仍需要（Pydantic 2.x 原生支持 UUID） |
| `actions/generator.py` | 可拆为 `dcc-mcp-codegen` 或 CLI 工具 |
| `actions/function_adapter.py` | 应移到 `dcc-mcp-rpyc` |

`type_wrappers.py` 和 `function_adapter.py` 明确为 RPyC 服务，不应在 core 中。

### 4.2 渐进式 Skill 发现机制需要增强

当前 Skill 发现是**启动时一次性扫描**。对于"渐进式发现和复用"的目标，需要：

1. **热重载**: 文件系统监听 + 增量注册（当前 mtime 缓存只在 `scan()` 调用时检查）
2. **远程 Skill Registry**: 支持从 HTTP/Git URL 拉取 Skills
3. **Skill 依赖声明**: 一个 Skill 可以声明依赖另一个 Skill
4. **Skill 版本管理**: 当前 `version: "1.0.0"` 只是元数据，没有实际语义

### 4.3 缺少 DCC Connection 抽象

core 中没有 "如何连接到 DCC" 的抽象。这意味着：
- `dcc-mcp-rpyc` 用 RPyC 连接
- 未来可能有 WebSocket、gRPC 等连接方式
- 但 core 没有提供 `DCCConnectionProtocol`

**建议**: 在 `protocols/` 中添加:
```python
@runtime_checkable
class DCCConnectionProtocol(Protocol):
    def connect(self) -> bool: ...
    def disconnect(self) -> None: ...
    def is_connected(self) -> bool: ...
    def execute_in_dcc(self, action_name: str, **kwargs) -> ActionResultModel: ...
```

### 4.4 `dcc-mcp`（顶层包）应作为聚合器而非框架

建议 `dcc-mcp` 仓库的角色:
- 提供 `dcc-mcp` CLI 命令 (list-dccs, list-actions, call-action)
- 聚合所有 DCC MCP Server 的入口
- 配置文件管理 (哪些 DCC 启用、端口分配等)
- **不应**包含业务逻辑

---

## 五、重构优先级建议

| 优先级 | 项目 | 影响 | 工作量 |
|--------|------|------|--------|
| P0 | 修复 Python 版本声明 (≥3.9) | 解放语言特性，修正错误声明 | 小 |
| P0 | ActionRegistry 去单例化或加 namespace 隔离 | 消除跨 DCC 污染 | 中 |
| P0 | 禁止覆写 `process()`，强化 `_execute()` 协议 | 防止用户跳过标准流程 | 小 |
| P1 | 移除 `call_action` 上的 `@error_handler` | 消除双重异常包装 | 小 |
| P1 | 将 `type_wrappers` 和 `function_adapter` 移到 rpyc 包 | 核心层更薄 | 中 |
| P1 | 定义 `DCCAdapterProtocol` | Skills 系统可测试性 | 小 |
| P1 | `ActionContext` 类型化 | IDE 友好、类型安全 | 中 |
| P2 | `MCPServerBuilder` 工厂 | 减少下游重复代码 | 中 |
| P2 | `_execute()` 改为返回 `OutputModel` | 消除副作用设计 | 大 |
| P2 | 事件名常量化 | 防止拼写错误 | 小 |
| P2 | Skills 包独立化 | 可选安装 | 中 |
| P3 | 能力协商 + 版本协商 | 生态可扩展性 | 中 |
| P3 | Skill 热重载 + 远程 Registry | "渐进式发现"核心能力 | 大 |

---

## 六、总结

### 优点
1. **类型化的 Action 系统** — Pydantic InputModel/OutputModel 提供了强验证和自动 JSON Schema 生成
2. **MCP 协议对齐** — protocols 包完整映射 MCP 2025-11-25 规范
3. **Skills 零代码注册** — SKILL.md + scripts/ 模式让非开发者也能贡献工具
4. **中间件 + 事件系统** — 可扩展的横切关注点处理
5. **测试覆盖较高** (94%) — 基础设施可信赖

### 核心风险
1. **单例注册表** — 多 DCC 共存场景下会出问题
2. **Action 生命周期** — `setup()` → `_execute()` → `process()` 三段式容易误用
3. **核心层过厚** — RPyC 相关代码不应在 core 中
4. **缺少连接抽象** — core 没有定义如何连接 DCC
5. **Python 版本** — 声称支持 3.7 但实际不行

**建议**: 趁尚未投入生产，优先处理 P0 项目（约 1-2 天工作量），再逐步推进 P1（约 1 周）。P2/P3 可随功能迭代逐步完成。
