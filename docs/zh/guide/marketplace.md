# Marketplace：技能包目录与安装器

Marketplace 是一个 CLI 优先的发现和安装系统，用于官方和社区技能包。它将人类可读的名称从一个或多个目录源中解析，下载或克隆匹配的包，并注册到系统中，使得 DCC 适配器能够在下次重启或调用 `reload_skill_paths` 时发现它们。

## 架构

```
┌──────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  CLI (CLAP)  │ ──▶ │ 应用层           │ ──▶ │ 领域层           │
│ marketplace  │     │ marketplace.rs   │     │ marketplace.rs   │
│ 子命令        │     │ (业务逻辑)       │     │ (类型/源)        │
└──────────────┘     └──────────────────┘     └──────────────────┘
       │                        │                       │
       │                        ▼                       │
       │              ┌──────────────────┐              │
       │              │ dcc-mcp-catalog  │              │
       └──────────────│ (解析/搜索)      │──────────────┘
                      └──────────────────┘
                              │
                              ▼
                     ┌──────────────────┐
                     │  Gateway         │
                     │  gateway://catalog│
                     │  MCP 资源        │
                     └──────────────────┘
```

市场代码分为三层：

1. **领域层**（`crates/dcc-mcp-cli/src/domain/marketplace.rs`）— 类型定义：
   `MarketplaceSource`、`MarketplaceHit`、`MarketplaceSearchResult`、
   `InstalledMarketplacePackage`、`OutdatedMarketplacePackage` 等。

2. **应用层**（`crates/dcc-mcp-cli/src/application/marketplace.rs`）—
   业务逻辑：源管理、跨源搜索、安装、卸载、更新检查。

3. **目录包**（`crates/dcc-mcp-catalog/`）— 独立包，解析 `marketplace.json` /
   `catalog.yml` 文件，按关键词和 DCC 类型搜索条目，并查看单个条目的详情。

网关还通过 MCP 资源（`gateway://catalog`）暴露目录数据，缓存周期为 5 分钟（参见 [catalog.md](catalog.md)）。

## 源

Marketplace **源**是指向目录文件的命名引用。源持久化存储在 `~/.dcc-mcp/marketplace/sources.json` 中。

| 源类型              | 示例                                          |
|---------------------|-----------------------------------------------|
| 官方（内置）        | `dcc-mcp/marketplace`                         |
| GitHub slug         | `my-org/my-skills`                            |
| 原始 JSON URL       | `https://example.com/catalog.json`            |
| 本地文件            | `/path/to/local-catalog.yml`                  |

### 源优先级

1. 内置官方源（`dcc-mcp/marketplace`）
2. 用户配置的源（持久化在 `sources.json`）
3. 环境变量源（`DCC_MCP_MARKETPLACE_SOURCES`）
4. 显式 `--source` CLI 标志

设置 `DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES=1` 可禁用内置源。

## CLI 命令

| 命令                                      | 描述                      |
|-------------------------------------------|---------------------------|
| `marketplace add <source>`                | 注册一个市场源            |
| `marketplace list`                        | 列出已配置的源            |
| `marketplace search --query <q>`          | 跨源搜索条目              |
| `marketplace inspect <name>`              | 显示完整条目元数据        |
| `marketplace install <name> --dcc <dcc>`  | 安装技能包                |
| `marketplace list-installed --dcc <dcc>`  | 列出已安装的包            |
| `marketplace uninstall <name> --dcc <dcc>`| 移除已安装的包            |
| `marketplace outdated [name] --dcc <dcc>` | 检查是否有更新版本        |
| `marketplace update [name] --all`         | 升级已安装的包            |

完整参数参考：[cli-reference.md](cli-reference.md#marketplace)。

## 安装类型

支持三种安装类型，由目录条目的 `install.type` 字段控制：

### Git（`install.type: git`）

安装时克隆仓库，后续更新时使用 `git fetch && git checkout <ref>`。
最适合活跃开发的技能包。

```yaml
- name: dcc-mcp-maya-skills
  install:
    type: git
    url: "https://github.com/example/dcc-mcp-maya-skills.git"
    ref: "v1.2.0"
```

### Zip（`install.type: zip`）

下载 ZIP 存档（从 URL 或本地路径）并解压。支持 `sha256` 校验。
存档根目录必须包含恰好一个顶层目录，该目录会自动展平。

```yaml
- name: dcc-asset-hunyuan-download
  install:
    type: zip
    url: "https://example.com/packages/hunyuan-v2.zip"
    sha256: "a1b2c3d4e5f6..."
```

### Path（`install.type: path`）

从本地目录复制文件。适用于开发或内部工具。

```yaml
- name: my-internal-skills
  install:
    type: path
    url: "/share/skills/my-internal-skills"
```

## 目录布局

已安装的包存放于：

```
~/.dcc-mcp/marketplace/
├── sources.json              # 注册的源列表
├── installed.json            # 已安装包的状态
├── maya/
│   ├── dcc-mcp-maya-skills/  # 已安装的 git 克隆
│   └── my-custom-skill/      # 已安装的路径复制
└── blender/
    └── dcc-blender-skills/
```

DCC 适配器会自动将 `~/.dcc-mcp/marketplace/<dcc>` 加入其技能搜索路径
（参见 `server_base.py` 中的 `collect_skill_search_paths()`），因此
已安装的技能会在适配器启动或 `reload_skill_paths` 时被发现。

## 环境变量

| 变量                                      | 默认值                                     | 描述                        |
|-------------------------------------------|--------------------------------------------|-----------------------------|
| `DCC_MCP_MARKETPLACE_SOURCES`             | 未设置                                     | 逗号分隔的额外源            |
| `DCC_MCP_MARKETPLACE_SOURCES_FILE`        | `~/.dcc-mcp/marketplace/sources.json`      | 源持久化路径                |
| `DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES`  | 未设置                                     | 禁用内置官方源              |
| `DCC_MCP_MARKETPLACE_INSTALL_ROOT`        | `~/.dcc-mcp/marketplace`                   | 安装根目录覆盖              |
| `DCC_MCP_MARKETPLACE_OFFLINE`             | 未设置                                     | 强制仅本地目录模式          |
| `DCC_MCP_MARKETPLACE_CATALOG_URL`         | 官方市场 URL                               | 覆盖远程目录 URL            |

## 安全

- **路径遍历保护**：`marketplace_path_component()` 拒绝空组件、`.`、`..`、
  前导点和非 ASCII 字母数字字符。
- **SHA256 校验**：ZIP 安装会验证 `install.sha256`（如果存在），在哈希不匹配
  时拒绝安装，且不会修改已有包。
- **压缩包逃逸检测**：ZIP 解压会拒绝逃脱安装根目录的条目。
- **强制模式**：`--force` 会在安装失败时重试，但当替换本身失败时会保留
  现有包。

## 网关集成

网关通过 MCP 资源暴露目录数据：

```python
# 搜索所有目录条目
result = client.resources_read("gateway://catalog?query=physics")

# 按精确名称查看单个条目
result = client.resources_read("gateway://catalog/dcc-mcp-physics-sim")
```

网关以 5 分钟缓存周期获取远程 `marketplace.json`，在离线时回退到本地
`dcc-mcp-catalog.yml`。设置 `DCC_MCP_MARKETPLACE_OFFLINE=1` 可强制
仅本地模式。

## 目录条目格式

```yaml
- name: dcc-mcp-maya-skills          # 唯一 kebab-case 标识符
  description: "Official Maya skill pack"
  dcc: [maya]                        # 支持的 DCC 类型
  url: "https://github.com/..."      # 项目 URL
  tags: [skills, maya, official]     # 可搜索标签
  version: "1.2.0"                   # 当前版本
  min_core_version: ">=0.17.0"       # 最低 dcc-mcp-core 版本
  install:
    type: git                        # git | zip | path
    url: "https://github.com/..."
    ref: "v1.2.0"                    # 标签/分支/提交（git 类型）
    sha256: "a1b2c3..."              # 内容哈希（zip 类型）
  maintainer: "team@example.com"     # 可选联系方式
```

## 参见

- [cli-reference.md](cli-reference.md) — 带完整标志文档的 CLI 命令参考
- [catalog.md](catalog.md) — DCC-MCP 公共适配器目录格式
- [skills.md](skills.md) — 如何编写技能包
- [admin-ui.md](admin-ui.md) — Web 仪表盘中的市场面板
