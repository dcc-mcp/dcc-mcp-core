# Adapter 启动计划与就绪契约

这是 `dcc-mcp-cli start-instance` 背后的 adapter 作者契约。如果你维护一个
DCC adapter，并希望 agent 能从「没有实例在运行」直接推进到「typed session
就绪」，而不需要人工打开宿主，请读这一篇。

职责划分是固定的，也正是这套设计的核心：

| 层 | 归属 | 内容 |
|----|------|------|
| 生命周期 | Core | 授权、项目绑定、复用、派生进程、等待注册、等待就绪、超时、受保护的停止 |
| 就绪语义 | Core | 就绪位与阻塞状态分类 |
| 启动计划 | **Adapter** | 对哪个项目运行哪个可执行文件、使用哪些 argv |
| 运行时 metadata | **Adapter** | operation ID、项目绑定、窗口句柄、宿主进度、阻塞状态 |
| 就绪取值 | **Adapter** | 该宿主的 `/v1/readyz` 报告什么 |

Core 从不自行拼出 DCC 专属命令行。没有启动计划时，操作会以
`blocking_state: launch_plan_missing` 失败关闭，而不是猜测一个可执行文件；
它也从不下载、安装、升级或修改配置。

命令层用法见 [cli-reference.md](cli-reference.md)。本文档只讲**你的 adapter
需要发布什么**。

## 零实例缺口

即使某个 DCC 的 adapter 已安装并针对精确项目生成了 receipt，只要没有宿主
进程在跑，`dcc-mcp-cli list` 就会报告 `total: 0`，agent 此时没有任何结构化
的前进路径。

`dcc-mcp-cli dcc-types --dcc-type <dcc> --project <path>` 在这种状态下现在会
推荐 `start-instance` —— 但**前提是该 DCC 类型与项目能解析出一个经过校验的
启动计划**。在你的 adapter 发布计划之前，推荐结果会退回 `install`，缺口依然
存在。

## 第 1 步 — 发布启动计划

### Core 在哪里查找

解析顺序是确定的，最具体的来源优先。命中第一个来源后不再查找后续来源。

| 顺序 | 位置 | 报告的 `launch_plan_source` |
|------|------|------------------------------|
| 1 | 命令行 `--launch-plan <path>` | `explicit` |
| 2 | `<project>/.dcc-mcp/launch-plan.json` | `project_receipt` |
| 3 | `<registry_dir>/launch-plans/<dcc_type>.json` | `state_registry` |

`<registry_dir>` 取自 `$DCC_MCP_REGISTRY_DIR`，缺省回落到
`<temp>/dcc-mcp-registry`。state registry 的文件名是 DCC 类型归一化后的结果：
转小写，空格与连字符替换为下划线，因此 `3ds Max` 对应 `3ds_max.json`。

当可执行文件依赖具体项目时按项目发布（来源 2）——例如 Unity 项目锁定某个
Editor 版本、Blender 文件绑定某个构建。当一个安装服务所有项目时按机器发布
（来源 3）。两者可以共存，项目 receipt 优先。

### Schema v1

```json
{
  "schema_version": 1,
  "dcc_type": "unity",
  "executable": "C:/Program Files/Unity/Hub/Editor/2022.3.10f1/Editor/Unity.exe",
  "argv": ["{executable}", "-projectPath", "{project}", "-noUpmPrefetch"],
  "version": "2022.3.10f1",
  "project_markers": ["ProjectSettings/ProjectVersion.txt"],
  "cwd": null
}
```

| 字段 | 类型 | 必填 | 含义 |
|------|------|------|------|
| `schema_version` | 整数 | 否 | 缺省为 `1`。**大于** `1` 的值会被拒绝 |
| `dcc_type` | 字符串 | 是 | 必须等于请求的 `--dcc-type`，不区分大小写比较 |
| `executable` | 字符串 | 是 | 绝对路径，且启动时必须**作为文件存在** |
| `argv` | 字符串数组 | 是 | 非空。`argv[0]` 展开后必须恰好等于 `executable` |
| `version` | 字符串 | 否 | 宿主版本。传入 `--version` 时按精确字符串比较 |
| `project_markers` | 字符串数组 | 否 | 项目内必须存在的相对路径 |
| `cwd` | 字符串 | 否 | 派生宿主的工作目录。未设置或不是目录时回落到项目根目录 |

第二个例子：按机器发布，来自另一个宿主家族。

```json
{
  "schema_version": 1,
  "dcc_type": "blender",
  "executable": "/usr/local/blender/4.2/blender",
  "argv": ["{executable}", "--python-expr", "import dcc_mcp_blender.bootstrap; dcc_mcp_blender.bootstrap.serve()", "{project}"],
  "version": "4.2.1",
  "project_markers": [],
  "cwd": null
}
```

### 会失败关闭的校验规则

下面每条规则都会以结构化报告终止操作，没有任何一条会回落到猜测的命令行。

| 规则 | `blocking_state` |
|------|------------------|
| `schema_version` 大于支持的版本 | `invalid_launch_plan` |
| `dcc_type` 与请求的 DCC 类型不一致 | `invalid_launch_plan` |
| JSON 无法读取或格式错误 | `invalid_launch_plan` |
| `argv` 为空 | `invalid_launch_plan` |
| 展开后 `argv[0]` 与 `executable` 不是逐字节相同 | `invalid_launch_plan` |
| `executable` 不是绝对路径，或不是已存在的文件 | `missing_executable` |
| 传入了 `--version` 且与 `version` 不完全相等 | `version_mismatch` |
| 声明的 `project_markers` 某项在磁盘上不存在 | `project_marker_missing` |
| `--project` 不是已存在的目录 | `project_not_found` |
| 三个来源都没有解析到计划 | `launch_plan_missing` |
| 操作员未传 `--yes` | `authorization_required` |

`project_markers` 是 Core 拒绝启动错误宿主的手段。Unity 用
`ProjectSettings/ProjectVersion.txt`；对于没有项目文件的宿主，宁可留空也不要
编造一个弱标记。

## 第 2 步 — argv 占位符

Core 在派生进程前对每个 argv 元素展开这些 token：

| 占位符 | 展开为 |
|--------|--------|
| `{executable}` | `executable` 路径，原样 |
| `{project}` | 归一化后的项目根目录 |
| `{dcc_type}` | 归一化后的 DCC 类型（小写） |
| `{version}` | 计划里的 `version`，或 `--version`，否则为空字符串 |

需要留意的规则：

- 展开是纯字符串替换，顺序为 executable、project、dcc type、version。没有
  shell：token 原样交给操作系统，因此不能依赖引号、通配或 `&&`。
- 展开后 `argv[0]` 必须与 `executable` **逐字节一致**。如果计划把 `argv[0]`
  展开成同名的另一个目录里的二进制文件，会被拒绝。Core 校验的是它记录过的
  路径，不会去派生另一个。
- `{project}` 拿到的是归一化后的绝对路径。Windows 上会去掉 `\\?\` 前缀，
  保证值对操作员可读。
- 由于展开是有序的，路径本身若含有 `{project}`、`{dcc_type}` 或
  `{version}`，会被二次展开。请避免发布含这些字面量的可执行文件或项目路径。

## 第 3 步 — 交给宿主的环境变量

Core 以 detached 方式派生宿主，把 stdin/stdout/stderr 重定向到空，并设置三个
变量，让 adapter 能把整个生命周期串起来：

| 变量 | 值 |
|------|-----|
| `DCC_MCP_START_OPERATION_ID` | operation ID，从启动到终态就绪全程稳定 |
| `DCC_MCP_START_PROJECT` | 归一化后的项目根目录 |
| `DCC_MCP_START_DCC_TYPE` | 归一化后的 DCC 类型 |

**你的 adapter 必须把 `DCC_MCP_START_OPERATION_ID` 抄进自己的注册行。** 这是
Core 把新注册的实例绑定回派生它的那个 operation 的唯一依据 —— 崩溃后 PID 会被
复用，所以 operation ID 优先，PID 只是第二选择。

`start-instance` 不会设置 `DCC_MCP_LAUNCH_ID`。该字段与 `role` 字段是
RFC-0007 §3.2 既有的身份契约，已经提供了按 launch 分组以及区分宿主与 sidecar
的能力。「复用既有实例而不重复启动」这一行为不需要你新增任何东西。

## 第 4 步 — 写注册行 metadata

本节的所有内容都是 Core 从 FileRegistry 行里读回来的。有三种机制可用，除
特别说明外都是 `str -> str` 的 metadata：

1. `McpHttpConfig.instance_metadata` —— 在 `server.start()` 之前设置。适合
   启动期就已知的值，例如 operation ID 与项目路径。
2. `handle.update_gateway_metadata({...})` —— 运行时合并；空字符串表示清除该
   key。适合进度与阻塞状态。
3. `handle.update_gateway_extras({...})` / `McpHttpConfig.instance_extras`
   —— JSON 类型版本，用于数字、布尔和嵌套值。

`DccServerBase.update_gateway_metadata()` **不是**这里该用的调用：它只接受
`scene`、`version`、`documents` 和 `display_name`。要写下面的 key，请用配置上
的 `instance_metadata` 或裸 handle。

Core 先在 `metadata` 里查找每个 key，再去 `extras`，因此对字符串型 key 来说
两种机制都可用。

### 身份与绑定

| 用途 | key，按查找顺序 |
|------|------------------|
| operation 绑定 | `dcc_mcp_operation_id`、`operation_id`、`dcc_mcp.operation_id` |
| 项目绑定 | `dcc_mcp_project`、`project`、`dcc_mcp.project` |

**两者都是承重的。** 复用、收敛和受保护的停止都靠它们匹配。一个两者都不写的
adapter 会遇到「第二次相同的 `start-instance` 又启动了一个宿主」，因为 Core
没有任何东西可以认出第一个。

### 原生窗口句柄

`window_handle`、`native_window_handle`、`hwnd`、
`dcc_mcp_window_handle`、`dcc_mcp.window_handle`。可选，会作为 `window_handle`
出现在报告里。注意 Core 不会替你发布它：`DccServerBase` 会在内部解析
`dcc_window_handle` 用于 UI Control 作用域绑定，但该值不会写入注册行。想让
它被报告出来，就得自己发布。

### 宿主进度（仅诊断用）

`compiling`、`importing`、`refreshing`、`updating`、`play_mode`、
`domain_reload`、`progress_stage`、`progress_phase`、`progress_message`。

Core 会把其中存在的值原样、不解释地作为 `host_progress` 呈现在报告里。
**它们只作诊断，Core 绝不用它们把门禁卡住。** 上报 `compiling: "true"` 既不
延长超时，也不推迟终态报告；只有第 5 步里 `/v1/readyz` 的位才有这个作用。用
它们解释宿主**为什么**还没就绪，而不是用来申请更多时间。

### 阻塞状态（见第 6 步）

`restart_required` / `dcc_mcp_restart_required` / `dcc_mcp.restart_required`、
`blocking_dialog` / `modal_dialog` / `dcc_mcp_blocking_dialog` /
`dcc_mcp.modal_dialog`、`project_lock` / `project_locked` /
`dcc_mcp_project_lock` / `dcc_mcp.project_lock`、`license_state` /
`license_status` / `dcc_mcp_license_state` / `dcc_mcp.license_state`，以及
`failure_stage` 与 `failure_reason`。

### 结构化诊断

`failure_stage`、`failure_reason`、`failure_at_unix`、`host_rpc_uri`、
`host_rpc_scheme`、`sidecar_pid`、`gateway_health_url`、
`gateway_recovery_driver`、`registration_refresh_mode`、
`gateway_guardian_active`、`gateway_guardian_failures`、
`gateway_guardian_restarts`，以及日志路径 key（`sidecar_log_dir` /
`stdio_log_dir` / `log_dir` 及对应的 `*_stdout_path` / `*_stderr_path`）。这些
会填充报告里的 `diagnostics`。

### 示例

```python
import os

from dcc_mcp_core import McpHttpConfig


def build_config(project: str) -> McpHttpConfig:
    config = McpHttpConfig(dcc_type="unity")
    metadata: dict[str, str] = {}

    operation_id = os.environ.get("DCC_MCP_START_OPERATION_ID")
    if operation_id:
        metadata["dcc_mcp_operation_id"] = operation_id

    # 优先使用 Core 注入的环境变量，回落到 adapter 自己的项目根。
    metadata["dcc_mcp_project"] = os.environ.get("DCC_MCP_START_PROJECT", project)

    config.instance_metadata = metadata
    return config


# 之后，当宿主进入编译阶段：
handle.update_gateway_metadata({
    "progress_stage": "compiling",
    "compiling": "true",
})

# 编译结束后：
handle.update_gateway_metadata({
    "progress_stage": "",
    "compiling": "",
})
```

## 第 5 步 — 通过 `/v1/readyz` 上报就绪状态

`start-instance --wait-ready` 会轮询实例的 `/v1/readyz`，直到所有必需的位都
为 `true` 或超时。

| 位 | 含义 |
|----|------|
| `process` | HTTP listener 有响应 |
| `dcc` | 宿主已完成初始化 |
| `skill_catalog` | search/load 元数据可用 |
| `dispatcher` | action dispatcher 已接好 |
| `host_execution_bridge` | 已挂接宿主执行桥 |
| `main_thread_executor` | 桥上有正在运行的主线程 pump |

可以依赖的语义：

- 默认要求 `process,dcc,skill_catalog,dispatcher`。用 `--require` 修改；值会被
  转为小写，连字符折叠为下划线。
- 一个位**只有布尔 `true` 才算满足**。其他任何类型，包括字符串 `"true"`，
  都视为缺失。
- `host_execution_bridge` 与 `main_thread_executor` 刻意不在默认集合里。校验
  仅限主线程的 tool 时，用
  `--require host_execution_bridge,main_thread_executor` 显式加上。
- 若要求 `skill_catalog` 而 `/v1/readyz` 报告为 `false`，Core 会去探该实例的
  discovery MCP 的 `tools/list`，找 `search_tools`、`search_skills` 或
  `load_skill`。若存在其一，该位会被提升为 `true` 并附
  `skill_catalog_source: "discovery_mcp"`。暴露了这些 tool 的 adapter，即使
  就绪载荷里没写这个位，也能拿到认可。

## 第 6 步 — 阻塞状态

Core 会把你的 metadata 归类为一个终态，并始终附带一个非交互的
`next_action`，其中包含精确的 `command` 数组。

| `blocking_state` | adapter 如何触发 | `retryable` | 安全下一步 |
|------------------|------------------|-------------|-----------|
| `none` | 已达终态就绪 | 否 | 继续 `search` / `describe` / `call` |
| `restart_required` | `restart_required` 为真，或 `failure_stage`/`failure_reason` 含 `restart` 一词 | 否 | 停止自己拥有的实例，再重新启动 |
| `project_lock` | `project_lock` / `project_locked` 为真，或含 `lock`、`locked` 等词 | 是 | 关掉另一个宿主或换项目，然后重试 |
| `license` | `license_state` / `license_status` 不是 `valid`、`ok`、`active`，或含 `license` 一词 | 否 | 在宿主 UI 里解决授权，然后重试 |
| `modal_dialog` | `blocking_dialog` / `modal_dialog` 为真，或含 `dialog`、`modal` 等词 | 是 | 在宿主 UI 里关掉对话框，然后重试 |
| `adapter_bootstrap` | `failure_stage` / `failure_reason` 含 `bootstrap` 或 `sidecar` | 是 | 查看实例诊断，然后重试 |
| `missing_executable` | 计划里的 `executable` 已不存在或不是文件 | 否 | 重跑 adapter 安装，让 receipt 指向真实可执行文件 |
| `version_mismatch` | `--version` 与计划的 `version` 不相等 | 否 | 不带 `--version` 启动，或安装指定版本 |
| `ambiguous_reuse` | 多个在线实例声明同一个项目 | **否** | 停掉多余实例，或传 `--instance-id` |
| `launch_plan_missing` | 没有解析到计划 | 否 | 安装 adapter，让它发布经过校验的计划 |
| `authorization_required` | 未传 `--yes` | 否 | 操作员确认后加 `--yes` 重跑 |
| `timeout` | 操作在到达就绪前耗尽 `--timeout-secs` | 是 | 用更大的超时重试，或查看诊断 |
| `cancelled` | 操作员取消 | 否 | 等可以启动宿主时再跑 |
| `invalid_launch_plan` | 计划格式错误或不安全 | 否 | 重装 adapter 以重新发布计划 |
| `project_not_found` | `--project` 不是已存在的目录 | 否 | 传入精确的绝对项目根目录 |
| `project_marker_missing` | 声明的 `project_markers` 某项不存在 | 否 | 确认该项目的确是这个 adapter 所安装的项目 |
| `launch_failed` | 派生可执行文件失败 | 是 | 检查可执行文件路径与 OS 错误，然后重试 |

两个值得记住的细节：

**`ambiguous_reuse` 刻意不可重试。** 它的恢复动作是让操作员停掉某个实例或传
`--instance-id`。原样重放同一请求会永远复现同样的歧义，所以把它标成可重试会
诱导 agent 在一个不可能成功的请求上空转。其他标了 `retryable: true` 的状态，
在人工清除底层条件后都可以原样重放。

**自由文本只按整词匹配。** `failure_stage` 与 `failure_reason` 会按非字母数字
字符切词，并按完整单词比较。`blocked` 和 `unlocked` **不会**被归类为项目锁，
`lockfile` 也不会。`sidecar_bootstrap` 会命中，因为它被切成 `sidecar` 和
`bootstrap`。诊断信息请用平实的词点明状况，不要依赖子串。

结构化 key 先解释，然后才看自由文本。优先用结构化 key ——
`license_state: "expired"` 胜过一段散文式的 `failure_reason`。

## 第 7 步 — 收敛与受保护的停止

Core 为每个 operation 在 `<registry_dir>/start-instance/<operation_id>.json`
持久化一条记录，并用一个 `index.json` 把 `dcc_type|canonical-project` 映射到
拥有它的 operation ID。

收敛逻辑如下：一个在线实例可复用，当它上报了拥有方 operation ID，或其 PID
等于拥有方 operation 的 PID，或其项目绑定与请求的项目一致。通过这层过滤后若
仍有多个实例，可路由的优先；仍多于一个则为 `ambiguous_reuse`。

停止是独立且受保护的操作。`stop-instance --operation-id <id>` 在下列条件全部
满足时才会执行：

- 本地存在该 operation 记录；
- 该 operation 拥有（`owned`）这个进程 —— 即它自己派生的；
- 该 operation 已注册了 instance ID；
- 记录里的 DCC 类型匹配，且目标实例与拥有的一致。

只是收敛到别人已经在跑的宿主上的 operation 并不拥有它，也就不能停止它。这是
有意为之：`start-instance` 只停止它自己启动的东西。

## 操作员接口

```bash
# 只解析并报告计划，不派生任何进程。
dcc-mcp-cli start-instance --dcc-type unity --project /abs/path/MyProject --dry-run

# 启动并等待终态就绪。
dcc-mcp-cli start-instance --dcc-type unity --project /abs/path/MyProject --wait-ready --yes

# 收紧就绪门禁，并给慢宿主更长时间。
dcc-mcp-cli start-instance --dcc-type unity --project /abs/path/MyProject \
  --wait-ready --require skill_catalog,host_execution_bridge --timeout-secs 600 --yes

# 多个实例声明同一项目时做消歧。
dcc-mcp-cli start-instance --dcc-type unity --project /abs/path/MyProject \
  --instance-id 3f2a1c9e --wait-ready --yes

# 只停止该 operation 自己启动并拥有的实例。
dcc-mcp-cli stop-instance --operation-id <operation-id>
```

| 参数 | 作用 |
|------|------|
| `--dcc-type <dcc>` | 目标 DCC 类型；与计划不区分大小写比较 |
| `--project <path>` | 绝对项目根目录；必须存在，会被归一化 |
| `--launch-plan <path>` | 显式计划文档，覆盖另外两个来源 |
| `--version <v>` | 按精确字符串相等锁定计划的 `version` |
| `--instance-id <id>` | 收敛到唯一实例，而不是任一项目匹配 |
| `--wait-ready` | 等待终态就绪，而不是停在执行注册 |
| `--require <bits>` | 逗号分隔的就绪位；默认 `process,dcc,skill_catalog,dispatcher` |
| `--timeout-secs <n>` | 总预算；默认 `300` |
| `--interval-secs <n>` | 轮询间隔；默认 `1` |
| `--dry-run` | 只解析并报告计划，不派生进程 |
| `--yes` | 操作员授权派生 GUI 进程 |

退出码：`ok: false` 的报告退出为 `Unavailable`；`--wait-ready` 返回
`ready: false` 时退出为 `Timeout`。`--dry-run` 按设计返回 `ready: false` 且
仍然成功退出 —— dry run 永远不会是超时。

不带 `--wait-ready` 时报告会停在 `stage: registration`，绝不会声称
`terminal`：**进程派生不等于 adapter 就绪。**

## Adapter 验收清单

- [ ] 安装流程会向项目写入 `.dcc-mcp/launch-plan.json`，或向机器写入
      `<registry_dir>/launch-plans/<dcc_type>.json`。
- [ ] `executable` 是绝对路径，且启动时作为文件存在。
- [ ] `argv` 非空，且 `argv[0]` 展开后恰好等于 `executable`。
- [ ] `project_markers` 指向只有该 DCC 的项目才有的文件，或留空。
- [ ] 宿主把 `DCC_MCP_START_OPERATION_ID` 抄进注册行的
      `dcc_mcp_operation_id`。
- [ ] 宿主在注册行发布 `dcc_mcp_project`。
- [ ] `/v1/readyz` 以布尔值上报 `process`、`dcc`、`skill_catalog`、
      `dispatcher`，且只在真的成立时才置为 `true`。
- [ ] 长耗时启动状态通过宿主进度 key 上报，并理解它们仅供诊断。
- [ ] 阻塞状况用结构化 key 发布，而不是散文式 `failure_reason`，且使用整词。
- [ ] `dcc-mcp-cli start-instance --dry-run` 能在真实项目上解析出计划。
- [ ] 第二次相同的 `start-instance` 返回 `reused: true` 与
      `converged_on_operation_id`，而不是再启动一个宿主。

## 已记录的尖锐点与已知缺口

以下是已发布契约的固有属性，不是需要悄悄绕过的缺陷。记录在此，避免 adapter
作者重复踩坑：

1. **没有 Python helper 负责写计划。** adapter 直接写 `launch-plan.json`。
   `dcc_mcp_core` 里没有生成或校验该文档的 API，因此 schema 打错只会在
   `start-instance` 运行时以 `invalid_launch_plan` 暴露。
2. **`DccServerBase.update_gateway_metadata()` 写不了这些 key。** 它只接受
   `scene`、`version`、`documents`、`display_name`。请用启动期的
   `McpHttpConfig.instance_metadata` 或运行时的
   `handle.update_gateway_metadata()`。
3. **`window_handle` 不会替你发布。** Core 在内部解析窗口句柄用于 UI Control
   作用域绑定，但不写入注册行。想让 `start-instance` 报告
   `window_handle`，adapter 必须自己发布。
4. **版本锁定是精确字符串相等。** `--version 2022.3` 匹配不上声明
   `2022.3.10f1` 的计划，也不支持范围或 semver。
5. **占位符展开是有序字符串替换。** 含 `{project}`、`{dcc_type}` 或
   `{version}` 的路径会被展开两次。请避免这类路径。
6. **宿主进度 key 永远不参与门禁。** 上报 `compiling: "true"` 既不延长超时，
   也不推迟终态报告。
7. **operation ID 与项目都不写的 adapter 无法收敛。** 第二次相同的
   `start-instance` 会再启动一个宿主。
