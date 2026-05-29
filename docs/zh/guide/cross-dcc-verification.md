# 跨 DCC 验证

> 证明由一个 DCC 通过 MCP 服务器生成的资产可以被第二个 DCC 消费、检查和验证。本页说明 `dcc-mcp-core` 提供的**契约**以及下游 DCC 仓库如何接入它。

## 为什么是契约而非实现

每个 DCC（Blender、Maya、Unreal、Photoshop、Houdini……）都有自己的原生导入 API、场景遍历原语以及对"网格"的定义。`dcc-mcp-core` 有意回避这片丛林：它只定义验证器结果的**形状**和每个下游实现应克隆的**技能模板**。保持形状精简（三个字段）是契约可移植的关键。

实际的 `import_and_inspect` 逻辑位于下游仓库：

| DCC | 仓库 | 技能名称（约定） |
|-----|------|----------------|
| Blender | `dcc-mcp-blender` | `blender-fbx-verifier` |
| Maya | `dcc-mcp-maya` | `maya-fbx-verifier` |
| Unreal | `dcc-mcp-unreal` | `unreal-fbx-verifier` |
| Photoshop | `dcc-mcp-photoshop` | `photoshop-psd-verifier` |

## `SceneStats` 契约

```python
from dcc_mcp_core import SceneStats

observed = SceneStats(
    object_count=1,
    vertex_count=482,
    has_mesh=True,
    extra={"dcc": "blender-3.6"},  # 可选的 DCC 特定扩展信息
)
```

| 字段 | 类型 | 含义 |
|------|------|------|
| `object_count` | `int` | 导入后看到的顶层对象数量 |
| `vertex_count` | `int` | 所有网格几何体的总顶点数 |
| `has_mesh` | `bool` | 当任何导入对象是多边形几何体时为 True |
| `extra` | `dict` | 自由形式的扩展信息，可序列化，核心比较时忽略 |

### 比较生产者与验证者

辅助方法 `SceneStats.matches()` 实现了核心认可的唯一比较语义：

- **严格**比较 `object_count`（结构不变量）
- **严格**比较 `has_mesh`（检测静默的空导入）
- **模糊**比较 `vertex_count`（默认 ±5%），因为 FBX 法线、UV 缝合和切线基变化可能在重新导入时分割顶点

```python
produced = SceneStats(object_count=1, vertex_count=482, has_mesh=True)
observed = verifier_skill__import_and_inspect("/tmp/sphere.fbx")

assert produced.matches(observed, vertex_tolerance=0.05), (
    f"往返漂移：期望 {produced}，得到 {observed}"
)
```

三个核心字段之外的扩展字段放入 `extra`。比较器不读取 `extra`，这样 DCC 特定的遥测数据就不会破坏往返断言。

## 编写验证器技能

使用 `dcc-mcp-skills-creator` 脚手架生成技能，再接入 `dcc_mcp_core.SceneStats`。为新 DCC 创建验证器：

1. 在下游仓库脚手架生成新的技能目录（如 `dcc-mcp-blender/skills/blender-fbx-verifier/`）
2. 编辑 `SKILL.md`：设置 `dcc: blender`（或类似值），并按仓库约定重命名技能
3. 将 `scripts/import_and_inspect.py` 的桩函数体替换为 DCC 原生导入 + 检查调用，返回包裹在 `skill_success(...)` 中的 `SceneStats.to_dict()` 负载
4. 在下游仓库添加 CI 作业：
   - 启动两个 DCC 进程（或一个生产者 + 一个验证者）
   - 运行生产者技能创建并导出资产
   - 对导出的文件运行新的验证器技能
   - 使用 `dcc_mcp_core.SceneStats` 断言 `produced.matches(observed)`

### 示例桩（Blender）

```python
import bpy

from dcc_mcp_core import SceneStats
from dcc_mcp_core.skill import skill_entry, skill_success


def main(params):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.fbx(filepath=params["file_path"])
    objects = list(bpy.context.scene.objects)
    meshes = [o for o in objects if o.type == "MESH"]
    vertex_count = sum(len(m.data.vertices) for m in meshes)

    stats = SceneStats(
        object_count=len(objects),
        vertex_count=vertex_count,
        has_mesh=bool(meshes),
        extra={"blender_version": bpy.app.version_string},
    )
    return skill_success("导入并检查完成", **stats.to_dict())


if __name__ == "__main__":
    skill_entry(main)
```

## 往返 CI 的位置

`dcc-mcp-core` **不**附带 Blender + FBX 往返作业。那属于拥有生产者或验证者二进制文件的下游仓库，因为只有这些仓库固定了 DCC 版本矩阵。核心断言的不变量仅限于形状，由 `tests/test_verifier_contract.py` 验证。

遵循此契约的下游仓库应添加如下 CI 作业：

```
1. 以无头模式启动生产者 DCC，加载生产者技能
2. mcpcall call: producer__create_sphere, producer__export_fbx
3. 以无头模式启动验证者 DCC，加载验证者技能
4. mcpcall call: verifier__import_and_inspect(/tmp/sphere.fbx)
5. Python 断言: SceneStats.from_dict(...).matches(produced)
```

## 常见问题

### 为什么只有三个字段？边界框、材质、动画怎么办？

进入核心契约的每个字段必须在所有 DCC 中永远意义相同。三个字段（对象/顶点/has-mesh）是我们能够严格定义和测试语义的集合。任何 DCC 特定的数据（材质、相机、动画帧数、边界框）都可以通过 `extra` 传递，无需核心承诺跨 DCC 的定义。

### 我可以在下游仓库中扩展 `SceneStats` 吗？

不要子类化。将下游特定数据放入 `extra`。如果某个字段在所有支持的 DCC 中真正通用，我们会在次要版本中将其提升到核心契约。

### 如果我的 DCC 不暴露顶点数怎么办？

返回 `vertex_count=0` 并在 `extra` 中添加说明。针对该 DCC 的往返测试应仅将 `has_mesh` 作为真正的断言。

## 相关文档

- [`dcc-thread-safety.md`](dcc-thread-safety.md) — 验证器技能在实时 DCC 会话中运行时所依赖的主线程分发器原语
- [`host-adapter.md`](host-adapter.md) — 下游仓库用于将验证器接入 DCC 空闲循环的 `HostAdapter` 基类
- [`skills.md`](skills.md) — 验证器模板所基于的 SKILL.md 格式
