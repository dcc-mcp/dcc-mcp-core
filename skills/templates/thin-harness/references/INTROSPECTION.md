# Introspection — <DCC> Runtime Namespace

> How to discover the live DCC API without reading vendor docs.
> Replace `<module>` with the actual DCC Python module (e.g. `maya.cmds`, `bpy.ops`, `hou`).

---

## List a module's public names

```python
import <module>
result = [n for n in dir(<module>) if not n.startswith("_")]
```

## Get a function's signature and docstring

```python
import <module>
help(<module>.<function_name>)
```

## Search for functions matching a pattern

```python
import <module>, re
pattern = re.compile(r"poly.*")
result = [n for n in dir(<module>) if pattern.match(n)]
```

## Use dcc_introspect__* tools (issue #426)

Once the `dcc-introspect` built-in skill is loaded:

```
dcc_introspect__list_module(module="<module>")
  -> {"names": [...], "count": N}

dcc_introspect__signature(qualname="<module>.<function>")
  -> {"signature": "...", "doc": "...", "flags": {...}}

dcc_introspect__search(pattern="poly.*", module="<module>", limit=20)
  -> {"hits": [{"qualname": "...", "summary": "..."}, ...]}
```

## DCC-specific introspection

Replace this section with DCC-specific discovery commands, e.g.:

- **Maya**: `cmds.help("polyCube")` for flag docs, `mel.eval("whatIs polyCube")` for source
- **Blender**: `bpy.ops.mesh.__dir__()`, `bpy.data.objects.bl_rna.properties`
- **Houdini**: `hou.nodeTypeCategories()`, `help(hou.Node)`
