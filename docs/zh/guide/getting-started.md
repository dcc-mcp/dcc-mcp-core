# 快速开始

## 安装

### 从 PyPI 安装

```bash
pip install dcc-mcp-core
```

### 从源代码安装

```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

## 环境要求

- **Python**: >= 3.7（CI 测试 3.11、3.12、3.13）
- **许可证**: MIT
- **依赖**: Python 3.8+ 零第三方依赖

## 快速上手

```python
from dcc_mcp_core import create_action_manager

# 为特定 DCC 创建动作管理器
manager = create_action_manager("maya")

# 执行动作
result = manager.call_action("create_sphere", radius=2.0)

# 检查结果
print(result.success, result.message, result.context)
```

## 开发环境设置

```bash
# 克隆仓库
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# 创建并激活虚拟环境
python -m venv venv
source venv/bin/activate  # Windows: venv\Scripts\activate

# 安装开发依赖
pip install -e .
pip install pytest pytest-cov pytest-mock pyfakefs

# 或使用 vx（推荐）
vx just install
```

## 运行测试

```bash
# 运行测试并生成覆盖率
vx just test

# 运行特定测试
vx uvx nox -s pytest -- tests/test_action_manager.py -v

# 代码风格检查
vx just lint

# 自动修复代码风格
vx just lint-fix
```

## 下一步

- 了解 [Actions 动作](/zh/guide/actions) — 核心构建块
- 探索 [Action Manager](/zh/guide/action-manager) 的生命周期管理
- 查看 [Skills 技能包](/zh/guide/skills) 的零代码脚本注册
