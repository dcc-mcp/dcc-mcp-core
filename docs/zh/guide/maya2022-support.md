# Maya 2022 / Python 3.7 长期支持策略

> **策略**：Python 3.7 是长期支持（LTS）兼容档位，不再按固定日期自动到期。
> 详细决策见 [ADR 011](../../adr/011-python-37-lts-compatibility-contract.md)。

## 支持承诺

Maya 2022 内嵌 CPython 3.7，很多仍在生产环境中的 DCC 版本也有同样约束。
因此，在完成 ADR 011 规定的正式弃用流程之前，`dcc-mcp-core` 会持续提供
可安装、可运行、经过门禁验证的 Python 3.7 路径。

机器可读的唯一事实来源是
[`compatibility/python.json`](../../../compatibility/python.json)。

## 支持的 wheel 档位

### 原生 Python 3.7

Linux 和 Windows 发布物包含原生 `cp37-cp37m` wheel，使用兼容契约锁定的
PyO3 0.28 系列构建。该 wheel 包含 `dcc_mcp_core._core`，提供正常的 Rust
加速与完整 Python API 表面。

这是 Python 3.7 兼容性的权威证明。

### py37-lite 回退

发布物同时包含不带 `_core` 的 `py3-none-any` wheel。对于没有原生 cp37
wheel 的平台，它提供纯 Python 的 host、skill、配置与 sidecar 回退能力。

`py37-lite` 是受支持的回退档位，但不能替代原生兼容证明。合并与发布门禁
必须同时要求原生档位和 lite 档位成功。

### Python 3.8+

维护中的高版本 Python 使用 `cp38-abi3` wheel。每个平台的一份稳定 ABI
构建覆盖 Python 3.8 到兼容契约声明的最高测试版本。

## 运行架构

```text
Maya 2022 / CPython 3.7
        |
        +-- 平台有原生 cp37 wheel
        |      `-- Python facade + Rust _core 扩展
        |
        `-- 平台没有原生 wheel
               `-- py37-lite facade + 外部 dcc-mcp-server sidecar
```

配套的 `dcc-mcp-server` wheel 携带平台二进制，而不是 Python 扩展 ABI，
因此 Python 3.7 可以安装它，lite 适配器也可以把网关执行交给 sidecar。

## CI 门禁

| 检查 | 频率 | 保证内容 |
| --- | --- | --- |
| `Python 3.7 contract` | 每个 PR | 包元数据、PyO3 系列、CI/发布任务、文档和测试版本与契约一致 |
| `Python 3.7 native (linux-x86_64)` | 每个 PR | 构建并安装原生 cp37 wheel，校验内容，执行运行时 smoke 和完整测试套件 |
| `Python 3.7 native (windows-x86_64)` | 每个 PR | 构建并安装原生 cp37 wheel，执行运行时 smoke |
| `Test (py37-lite)` | 每个 PR | 安装 lite wheel，在没有 `_core` 时验证回退行为 |
| `py37 syntax check` | 每个 PR | 使用真实 CPython 3.7 解析器编译 Python 源码与测试 |
| `Python 3.7 compatibility` | 每个 PR | 稳定聚合门禁；任何失败、跳过或取消都会失败 |

仓库 ruleset 应把精确任务名 `Python 3.7 compatibility` 配置为必需状态。

## 编码规则

- 所有需要进入 Python 3.7 的模块必须可导入；使用现代注解表达式时应启用
  postponed annotations。
- 发布代码不能使用 assignment expression、仅限位置参数、debug f-string、
  `match` 等 Python 3.8+ 语法。
- `compile()` 通过不等于运行时兼容；不能在 3.7 上直接求值现代注解。
- 对 Python 3.7 `typing` 中不存在的 `Protocol`、`Literal` 等运行时 API，
  使用 `_typing_compat` 或明确的回退实现。
- 需要支持 lite 档位的代码不能无条件导入 `_core`。
- 新增跨 Rust/Python 边界的公开 API 时，应覆盖原生和 lite 两种档位。

## 本地验证

vx 管理的 Python 3.7 足以执行语法门禁，但可能不包含 `pip` 或原生导入库。
构建并安装原生 wheel 需要一套完整的 CPython 3.7（包含 `pip` 与开发/导入库）。
Windows 上可使用 python.org 的标准安装。

PowerShell：

```powershell
$py37 = py -3.7 -c "import sys; print(sys.executable)"
vx just check-python-support
vx just check-py37-syntax
vx just build-py37 -i $py37
$wheel = Get-ChildItem dist/dcc_mcp_core-*-cp37-cp37m-*.whl | Select-Object -First 1
& $py37 scripts/ci/check_python_wheel.py --profile native_py37 --platform windows-x86_64 $wheel.FullName
& $py37 -m pip install --force-reinstall --no-deps $wheel.FullName
& $py37 scripts/ci/smoke_python37_runtime.py --profile native_py37
```

Bash：

```bash
PYTHON37="${PYTHON37:-python3.7}"
vx just check-python-support
vx just check-py37-syntax
vx just build-py37 -i "$PYTHON37"
"$PYTHON37" scripts/ci/check_python_wheel.py --profile native_py37 --platform linux-x86_64 \
  'dist/dcc_mcp_core-*-cp37-cp37m-*.whl'
wheel=$(ls dist/dcc_mcp_core-*-cp37-cp37m-*.whl | head -1)
"$PYTHON37" -m pip install --force-reinstall --no-deps "$wheel"
"$PYTHON37" scripts/ci/smoke_python37_runtime.py --profile native_py37
```

lite 档位继续使用同一个完整解释器。PowerShell：

```powershell
& $py37 scripts/build_py37_pure_wheel.py
$wheel = Get-ChildItem dist/dcc_mcp_core-*-py3-none-any.whl | Select-Object -First 1
& $py37 scripts/ci/check_python_wheel.py --profile lite_py37 --platform any $wheel.FullName
& $py37 -m pip install --force-reinstall --no-deps $wheel.FullName
& $py37 scripts/ci/smoke_python37_runtime.py --profile lite_py37
```

Bash：

```bash
"$PYTHON37" scripts/build_py37_pure_wheel.py
"$PYTHON37" scripts/ci/check_python_wheel.py --profile lite_py37 --platform any \
  'dist/dcc_mcp_core-*-py3-none-any.whl'
wheel=$(ls dist/dcc_mcp_core-*-py3-none-any.whl | head -1)
"$PYTHON37" -m pip install --force-reinstall --no-deps "$wheel"
"$PYTHON37" scripts/ci/smoke_python37_runtime.py --profile lite_py37
```

## 弃用流程

只有在以下条件全部满足后，才能移除 Python 3.7：接受一份替代 ADR、发布主版本、
至少提前 180 天公告，并给出受影响适配器的迁移路径。依赖升级或托管 runner
变化本身不能成为静默削弱兼容契约的理由。

## 参考

- [ADR 011：Python 3.7 LTS 兼容契约](../../adr/011-python-37-lts-compatibility-contract.md)
- [py37-lite 架构](../../guide/py37-lite-architecture.md)
- [适配器兼容矩阵](../../guide/adapter-compatibility-matrix.md)
