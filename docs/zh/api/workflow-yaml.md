# 工作流 YAML API

> **[English](../api/workflow-yaml.md)**

基于 YAML 声明式工作流定义。支持 task 与 step 两种语义，用于多步骤 DCC 工作流编排。与 Rust 端的 `WorkflowSpec`/`WorkflowStep` 互补，提供 YAML 文件加载和 MCP 工具注册。

**导出符号：** `WorkflowTask`, `WorkflowYaml`, `load_workflow_yaml`, `get_workflow_path`, `register_workflow_yaml_tools`

## WorkflowTask

数据类，表示工作流中的一个任务或步骤。

- `.name` — 任务名称
- `.kind` — `"task"` 或 `"step"`
- `.tool` — 调用的工具名
- `.inputs`, `.outputs` — 输入输出映射
- `.on_failure` — 失败处理策略
- `.interpolate_inputs(variables)` — 替换 `{{var}}` 模板

## WorkflowYaml

数据类，表示完整的工作流定义。

- `.name`, `.goal` — 名称与目标
- `.config`, `.variables` — 配置与变量
- `.tasks` — `WorkflowTask` 列表
- `.source_path` — YAML 文件路径
- `.validate()`, `.task_names()`, `.get_task(name)`, `.to_summary_dict()`

## 主要函数

- `load_workflow_yaml(path) -> WorkflowYaml` — 加载并验证工作流 YAML 文件
- `get_workflow_path(metadata) -> str | None` — 从 SkillMetadata 中提取工作流文件路径
- `register_workflow_yaml_tools(server, *, workflows=None, skills=None, dcc_name="dcc")` — 注册 `workflows.list` 和 `workflows.describe` MCP 工具

详见 [English API 参考](../api/workflow-yaml.md)。
