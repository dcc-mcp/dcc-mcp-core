# 取消机制 API

> **[English](../api/cancellation.md)**

协作式取消机制，用于在技能脚本和工具调度中支持请求级别的取消操作。基于 `contextvars` 实现，调度器自动设置 `CancelToken`，脚本通过 `check_cancelled()` 检查取消状态。

**导出符号：** `CancelToken`, `CancelledError`, `check_cancelled`, `set_cancel_token`, `reset_cancel_token`, `current_cancel_token`

## CancelToken

线程安全的取消标志。由调度器在请求上下文中安装。

- `.cancel()` — 标记为已取消
- `.cancelled -> bool` — 是否已取消

## CancelledError

当活动请求被取消时，由 `check_cancelled()` 抛出的异常。

## check_cancelled()

检查当前请求是否已被取消。如已取消则抛出 `CancelledError`，否则无操作（no-op）。在请求上下文外调用为安全空操作。

## set_cancel_token / reset_cancel_token

- `set_cancel_token(token) -> contextvars.Token` — 安装 CancelToken（仅调度器使用）
- `reset_cancel_token(reset) -> None` — 恢复先前的 token（与 `set_cancel_token` 配对）
- `current_cancel_token() -> CancelToken | None` — 返回当前活跃的 token

详见 [English API 参考](../api/cancellation.md)。
