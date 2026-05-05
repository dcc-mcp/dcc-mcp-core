# 上下文包（Context Bundles）

上下文包是 DCC MCP 会话解析后的运行时身份。它回答了：这属于哪个制作域、正在进行什么类型的工作、当前活跃的项目/镜头/资产是什么，以及哪些技能包应该可见？

```text
Rez 上下文 -> 环境变量 -> DCC 启动 -> 技能扫描 -> 网关元数据 -> 上下文感知的 tools/list
```

`dcc-mcp-core` 读取这个已解析的环境。它不选择包、解决版本，也不替代工作室包管理器。

## 运行时流程

1. Rez 为项目、部门、任务、资产类型和 DCC 解析包
2. 包命令设置 `DCC_MCP_*` 上下文和路径变量
3. DCC 适配器启动 `DccServerBase` 或 `McpHttpServer`
4. 从 `DCC_MCP_SKILL_PATHS` 和 `DCC_MCP_<DCC>_SKILL_PATHS` 中发现技能
5. 网关在 `FileRegistry` 中记录上下文元数据
6. 客户端对选定的上下文调用 `list_dcc_instances`、`search_skills` 和 `load_skill`，而不是一次性暴露所有工作室工具

## 元数据键

网关在每个实例的 `metadata` 字段下返回上下文元数据。内置的 `DccServerBase` 从环境变量填充这些键：

| 元数据键 | 环境变量 |
|---------|---------|
| `context_bundle` | `DCC_MCP_CONTEXT_BUNDLE` |
| `production_domain` | `DCC_MCP_PRODUCTION_DOMAIN` |
| `context_kind` | `DCC_MCP_CONTEXT_KIND` |
| `project` | `DCC_MCP_PROJECT` |
| `sequence` | `DCC_MCP_SEQUENCE` |
| `shot` | `DCC_MCP_SHOT` |
| `asset` | `DCC_MCP_ASSET` |
| `asset_type` | `DCC_MCP_ASSET_TYPE` |
| `task` | `DCC_MCP_TASK` |
| `toolset_profile` | `DCC_MCP_TOOLSET_PROFILE` |
| `package_provenance` | `DCC_MCP_PACKAGE_PROVENANCE` |
| `skill_paths` | `DCC_MCP_SKILL_PATHS` |
| `dcc_skill_paths` | `DCC_MCP_<DCC>_SKILL_PATHS` |

不使用 `DccServerBase` 的适配器可以通过 `McpHttpConfig.instance_metadata` 显式设置相同的值。

## 网关路由

网关本质上是对已启动上下文的选择器，不应将每个后端工具放大为一个庞大的全局接口。客户端应首先查看 `list_dcc_instances`，选择匹配的包或 DCC 会话，然后在该选定上下文中加载/搜索技能。

选择标准示例：

- `production_domain=film`、`context_kind=shot`、`task=animation` → Maya 动画 blocking 工具
- `production_domain=film`、`context_kind=shot`、`task=fx` → Houdini 缓存与模拟预览工具
- `production_domain=game`、`context_kind=level` → 关卡布局工具

## 示例

可复用的清单文件位于 `examples/context-bundles/`。示例 Rez 技能包位于 `examples/rez-skills/`，每个包含 `package.py`、`SKILL.md`、`tools.yaml`、`README.md` 以及一个小脚本。

包布局和来源指导，请参见 [rez-skill-packages.md](rez-skill-packages.md)。
