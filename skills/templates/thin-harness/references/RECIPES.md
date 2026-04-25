# Recipes — <DCC> Scripting

> Copy-pasteable snippets for common operations.
> Each section is an anchor — use `recipes__get(skill="<dcc>-scripting", anchor="...")`.

---

## example_operation

One sentence describing when to use this recipe.

```python
# Replace with a real DCC API call
result = some_module.some_function(arg1, arg2)
```

---

## list_selected_objects

List the names of currently selected objects.

```python
# Replace with the DCC-specific selection API
result = []  # e.g. cmds.ls(selection=True) in Maya
```

---

## create_primitive

Create a basic geometric primitive at the origin.

```python
# Replace with the DCC-specific primitive creation call
result = None  # e.g. cmds.polyCube(name="myCube")[0] in Maya
```

---

## set_transform

Set the world-space position of an object (absolute, not relative).

```python
# Replace with the DCC-specific transform API
# e.g. cmds.xform("myCube", translation=(1, 2, 3), worldSpace=True)
pass
```

---

## save_scene

Save the current scene to its current file path.

```python
# Replace with the DCC-specific save API
# e.g. cmds.file(save=True) in Maya
pass
```
