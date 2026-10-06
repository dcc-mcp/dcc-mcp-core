# Adapter 与 Core 的版本契约

Adapter 会把「支持哪个 `dcc-mcp-core` 版本」声明**两次**，而只有其中一份会被解析器强制执行。本文定义两份声明之间的契约、由第一份推导第二份的工具，以及两者不一致时失败的门禁。

## 两份声明为什么会漂移

| 声明 | 读取方 | 写入方式 | 是否强制 |
|------|--------|----------|----------|
| wheel 里的 `Requires-Dist: dcc-mcp-core>=0.19.3,<0.19.5` | `pip` | 由 `pyproject.toml` 生成 | 是 |
| `package.py` 里的 `requires = ["dcc_mcp_core-0"]` | 工作室的包环境解析器 | 手写 | 仅当它真的写了上界 |

包环境那份声明是手写的，会漂移。而且它是往**更宽**的方向漂：解析器完全可以挑一个 adapter 在 PyPI 上明确排除的 core 版本——`dcc_mcp_core-0` 允许 `0.x` 线上的任何版本。这样的环境顶层 import 全部成功，`import dcc_mcp_core`、`import dcc_mcp_maya`、`import dcc_mcp_maya.server` 都不报错，直到下一层才抛出一个完全没提到版本号的 `ImportError`。

这不是解析器的 bug，而是缺一份契约：从来没有人规定两份声明必须一致，也就从来没有人检查。

## 契约内容

机器可读的部分在
[`compatibility/adapter-core-requirement.json`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/compatibility/adapter-core-requirement.json)，
由 `scripts/ci/check_adapter_core_requirement.py` 强制执行。四条规则：

1. **Python 发行包元数据是真源。** 包环境那份声明由它推导，不能反过来。
2. **上界是必需的。** 没有上界的声明即使当前 core 恰好满足，也算违规。
3. **上界是排他的。** `<0.19.5` 渲染成 `..0.19.5` 区间记号；`<=0.19.5` 没有对应记号，直接拒绝，而不是悄悄放宽。
4. **包环境声明不得比已声明区间更宽。** 更窄是安全的，更宽就是解析器逃出区间的方式。

## 推导包环境声明

不要手写这个记号，从你已经在维护的那份声明生成它：

```python
from dcc_mcp_core.version_compat.core_requirement import requirement_from_pep440

requirement = requirement_from_pep440("dcc-mcp-core>=0.19.3,<0.19.5")
requirement.to_package_environment()   # 'dcc_mcp_core-0.19.3..0.19.5'
requirement.to_pep440()                # 'dcc-mcp-core>=0.19.3,<0.19.5'
```

| 包环境记号 | 含义 |
|------------|------|
| `dcc_mcp_core` | 任意版本——永远违规 |
| `dcc_mcp_core-0` | 任意 `0.x` 版本 |
| `dcc_mcp_core-0.19.3+` | `0.19.3` 及以上，无上界 |
| `dcc_mcp_core-0.19.3..0.19.5` | `0.19.3` 起，不含 `0.19.5` |

`..` 区间算子的上端是排他的，因此可以直接映射到 PEP 440 的 `<`。截断版本按前缀下界解读：`dcc_mcp_core-0` 等于 `>=0.0.0,<1.0.0`——这正是它「看起来有界、实际上什么都没排除」的原因。

## 发布时的检查

把门禁加进 adapter 的发布 workflow。当包环境声明比已声明区间更宽、声明无法解析、或 `package.py` 根本没声明 core 时，它会以非零码退出：

```yaml
- name: Check the core requirement contract
  run: python scripts/check_core_requirement.py --adapter-root . --github
```

按组织内共享 release-workflow digest kit 的同样方式，把
[`scripts/ci/check_adapter_core_requirement.py`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/scripts/ci/check_adapter_core_requirement.py)
复制进 adapter 仓库，这样 core 的 `main` 分支变更就不会改变 adapter 发布门禁的判定。该脚本只用标准库；解析规则来自已安装的 `dcc_mcp_core.version_compat.core_requirement`，因此两侧对「上界」的理解始终一致。

门禁从 `pyproject.toml` 读 `[project].dependencies`，从 `package.py` 读字面的 `requires` 列表。它用 `ast` 解析 `package.py` 且从不执行它，所以被扫描的仓库不会因为扫描而运行代码。`requires` 若不是字面字符串列表，会被报为错误，绝不当作豁免。

## 运行期尽早失败

希望硬失败的 adapter 在 import 时调用守卫：

```python
# dcc_mcp_maya/__init__.py
from dcc_mcp_core.version_compat.core_requirement import enforce_core_compatibility

enforce_core_compatibility(__name__)   # 读取 dcc-mcp-maya 自己的 Requires-Dist
```

`enforce_core_compatibility` 抛出 `CoreRequirementError`，其中写明 adapter、它声明的区间、正在运行的 core 版本以及修复办法。设置 `DCC_MCP_CORE_REQUIREMENT_ENFORCE=0` 可降级为警告。

`DccServerBase` 在构建每个 server 时会做同样的比较并输出启动警告，因此即使 adapter 还没有接入守卫，日志里也会出现点名版本的那一行。该检查是 best-effort——读不到 adapter 元数据时就没有可比较的对象——并且与版本溯源检查共用 `DCC_MCP_CORE_VERSION_CHECK=0` 开关。

## 上界策略：`<1.0.0` 不是上界

core 仍是 `0.x`，因此 `<1.0.0` 排除不了任何实际存在的版本。它只是 adapter 从兼容性矩阵里复制来的占位符，不是谁真的验证过的版本；而矩阵历史上建议的 `>=X.Y.0,<1.0.0` 恰恰让解析器把 adapter 和领先五个次版本的 core 配到了一起。

目标是**验证过的上界**：把 core 的上界钉在 adapter 已验证的最高版本之上的那个次版本。

```python
# pyproject.toml —— 已在 0.20.14 上验证，因此上界是 0.21
dependencies = ["dcc-mcp-core>=0.20.14,<0.21.0"]
```

```python
# package.py —— 由上一行推导，绝不手写
requires = ["dcc_mcp_core-0.20.14..0.21.0"]
```

门禁把开放的 `<1.0.0` 上界报为**警告**而非错误：兼容性矩阵里每一行目前都还在用它，一次性把严重级别翻成错误会让所有 adapter 同一次提交全红。新 adapter 应从一开始就使用验证过的上界；已有 adapter 在下次改动 pin 时迁移。

## 已知违规

`compatibility/adapter-core-requirement.json` 记录了观察到的、包环境声明比已声明区间更宽的 adapter。每一行保存声明区间、观察到的记号和契约要求的记号，门禁会从这两个输入重算漂移码——行的漂移码一旦对不上就是构建失败。

这些行在 core 的 CI 里是**警告**，因为修复属于 adapter 仓库；adapter 一旦对自身运行门禁，它们就变成错误。

## 相关文档

- [adapter-compatibility-matrix.md](../../guide/adapter-compatibility-matrix.md) —— core pin，以及 catalog 所镜像的矩阵行
- [adapter-release-checklist.md](../../guide/adapter-release-checklist.md) —— 契约检查在发布流程中的位置
- [rez-skill-packages.md](rez-skill-packages.md) —— 包布局与 Rez 包贡献的环境变量
- [`dcc_mcp_core.version_compat.core_requirement`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/python/dcc_mcp_core/version_compat/core_requirement.py) —— 解析、渲染与强制执行的 API
