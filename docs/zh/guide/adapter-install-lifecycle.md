# 适配器安装生命周期

适配器安装程序通常在其正在更新的 DCC 进程内运行。在 Windows 上，导入 `dcc_mcp_core._core` 会加载 `_core.pyd`；该原生模块会保持锁定状态直到进程退出，因此在删除适配器捆绑的包树时，卸载或升级可能会失败。

对于必须保持导入轻量级的安装程序和卸载程序代码，请使用 `dcc_mcp_core.install_lifecycle`。该模块仅使用 Python 标准库，不会导入 `_core`。

## Rez 或文件系统部署布局

流水线团队可以在包正式构建之前使用相同的引导脚本。首先解析包根目录，然后将返回的路径前置到启动伴生进程或网关的进程环境中：

```python
from dcc_mcp_core.install_lifecycle import resolve_deployment_layout

layout = resolve_deployment_layout(
    r"G:\_thm\rez_local_cache\ext",
    adapter_package="dcc_mcp_maya",
)

python_paths = layout["environment"]["prepend"]["PYTHONPATH"]
path_entries = layout["environment"]["prepend"]["PATH"]
```

当 Rez 激活时，辅助工具优先使用 `REZ_<PACKAGE>_ROOT` 变量，如 `REZ_DCC_MCP_CORE_ROOT`、`REZ_DCC_MCP_SERVER_ROOT` 和 `REZ_DCC_MCP_MAYA_ROOT`。没有 Rez 时，传递共享缓存根目录或显式 `package_roots` 映射：

```python
layout = resolve_deployment_layout(
    package_roots={
        "dcc_mcp_core": r"G:\_thm\rez_local_cache\ext\dcc_mcp_core",
        "dcc_mcp_server": r"G:\_thm\rez_local_cache\ext\dcc_mcp_server",
        "dcc_mcp_maya": r"G:\_thm\rez_local_cache\ext\dcc_mcp_maya",
    },
    adapter_package="dcc_mcp_maya",
)
```

这使开发、松散内部交付和打包的 Rez 部署保持在同一代码路径上。

## 导入轻量级预检

```python
from dcc_mcp_core.install_lifecycle import inspect_install_root

diagnostic = inspect_install_root(r"C:\Users\me\Documents\3dsMax\scripts\dcc_mcp_3dsmax")
if diagnostic["requires_restart"]:
    schedule_deferred_cleanup(diagnostic)
```

`inspect_install_root()` 检查当前进程中已加载的模块。如果安装根目录下的原生工件已加载，它会返回：

```json
{
  "status": "requires_restart",
  "requires_restart": true,
  "locked_path": "C:\\...\\dcc_mcp_core\\_core.pyd",
  "recommended_next_action": "Defer cleanup until the DCC host restarts, then remove or replace the install root."
}
```

## 注册表查询和伴生进程停止

安装程序可以检查共享的 FileRegistry，而无需创建任何 Rust 支持的对象：

```python
from dcc_mcp_core.install_lifecycle import query_runtime_state
from dcc_mcp_core.install_lifecycle import stop_runtime_entries

state = query_runtime_state(dcc_type="3dsmax", role="per-dcc-sidecar")
stop = stop_runtime_entries(dcc_type="3dsmax")
```

默认情况下，`stop_runtime_entries()` 仅定位发布 `metadata.sidecar_pid` 的行。除非显式传递 `include_host_processes=True`，否则不会终止父 DCC 进程。

## 混合运行时版本计划

网关可以同时看到多个 DCC 运行时。例如，Maya 可能仍在运行旧的伴生进程，而 3ds Max 已经启动了更新的版本。独立处理每个注册的实例，并从注册表元数据规划重启：

```python
from dcc_mcp_core.install_lifecycle import plan_runtime_updates
from dcc_mcp_core.install_lifecycle import query_runtime_state

state = query_runtime_state()
plan = plan_runtime_updates(
    state,
    target_versions={
        "core": "0.17.21",
        "server": "0.17.21",
        "adapter": "1.2.0",
    },
)
```

`ServiceEntry.version` 是 DCC 应用程序的版本，如 `Maya 2026` 或 `Photoshop 25.9`；它不是 `dcc-mcp-core` 包版本。运行时行必须通过元数据键发布包版本，如 `dcc_mcp_core_version`、`dcc_mcp_server_version` 和 `adapter_version`。当缺少包元数据时，`plan_runtime_updates()` 报告 `action=verify_runtime_metadata` 而不是将 DCC 应用版本视为包版本。

每个计划行报告组件漂移和重启操作：

```json
{
  "dcc_type": "maya",
  "action": "restart_sidecar",
  "restart_scope": "sidecar",
  "stale_components": ["core", "server", "adapter"],
  "recommended_next_action": "Stop the registered sidecar, restart it from the target deployment, then re-run MCP readiness."
}
```

当 `sidecar_pid` 存在时，管理界面应将 `action=restart_sidecar` 渲染为安全的伴生进程重启按钮。如果行报告 `manual_restart_required`，则运行时由宿主拥有，必须重启 DCC 进程后才能重置或期望 MCP 调用使用更新的代码。如果行报告 `verify_runtime_metadata`，则注册表行缺少足够的包版本元数据来安全决策；在假定其使用目标部署之前，请验证或重启该运行时。

在任何停止或重启之后，使用实例 MCP 端点验证就绪性，并在发送重置调用之前刷新网关注册表状态。

网关管理 JSON 已在每个实例上公开这些操作员提示：

```json
{
  "lifecycle": {
    "role": "per-dcc-sidecar",
    "owner": "release-smoke-test",
    "session": "test",
    "sidecar_pid": 31337,
    "supports_safe_stop": true,
    "safe_stop_url": "http://127.0.0.1:19000/safe-stop",
    "safe_stop_method": "POST",
    "restartable": true,
    "restart_command": "rez-env dcc_mcp_maya -- maya-sidecar"
  }
}
```

启动自己的 DCC 进程的发布冒烟测试应发布稳定的公共生命周期元数据（`owner`、`session`），并在支持时发布 `safe_stop_url` 回调。网关和 `dcc-mcp-cli stop-instance` 仅将安全停止请求转发到该显式回调，从不直接终止进程。

## 安全删除或替换

```python
from dcc_mcp_core.install_lifecycle import safe_remove_tree
from dcc_mcp_core.install_lifecycle import safe_replace_tree

removed = safe_remove_tree(install_root)
replaced = safe_replace_tree(staged_payload, install_root)
```

当预检通过时，两个辅助工具都会尝试立即清理。如果 Windows 报告原生文件锁定，结果会为延迟启动钩子进行结构化：

```json
{
  "status": "requires_restart",
  "requires_restart": true,
  "locked_path": "C:\\...\\_core.pyd",
  "reason": "windows_file_lock",
  "deferred_operation": {
    "operation": "remove_tree",
    "path": "C:\\...\\dcc_mcp_3dsmax"
  }
}
```

当 DCC 特定的安装程序需要仅 JSON 的控制路径时，从子进程运行相同的辅助工具：

```bash
python -m dcc_mcp_core.install_lifecycle inspect C:\path\to\adapter
python -m dcc_mcp_core.install_lifecycle stop --dcc-type 3dsmax
python -m dcc_mcp_core.install_lifecycle layout --cache-root G:\_thm\rez_local_cache\ext --adapter-package dcc_mcp_maya
python -m dcc_mcp_core.install_lifecycle plan-update --target-version core=0.17.21 --target-version server=0.17.21
python -m dcc_mcp_core.install_lifecycle remove C:\path\to\adapter
```
