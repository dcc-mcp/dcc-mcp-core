# Spatial Interchange Conventions

## Contract

Describe each space with explicit signed semantic axes and scale:

```text
right, up, forward: one each of +/-X, +/-Y, +/-Z
meters_per_unit: physical meters represented by one coordinate unit
```

Let `S_source` and `S_target` be matrices whose columns are the respective
right, up, and forward vectors. The axis conversion is:

```text
A = S_target * transpose(S_source)
position_scale = source.meters_per_unit / target.meters_per_unit
p_target = position_scale * A * p_source
```

The returned matrices are row-major values applied to column vectors. For a
4x4 transform `T`, change basis with `C * T * inverse(C)`, where `C` is the
returned `position_matrix`. Apply `A` to directions. Apply the inverse
transpose of `C` to normals, then normalize; because this contract permits
only orthogonal axes and uniform unit scale, normalized `A * normal` is
equivalent. If `det(A) < 0`, reverse triangle winding and update tangent-frame
handedness. Convert quaternions or matrices before deriving Euler angles.

## Do not guess from the application name

"Forward" may mean model front, camera view, navigation, or an FBX SDK axis
parity. Record which meaning the transfer requires. Prefer file metadata and
live settings over the common values below.

| System | Officially documented convention relevant to transfer |
| --- | --- |
| glTF 2.0 | Right-handed, +Y up, +Z forward, meters. |
| Blender | +Z up; its FBX operator exposes Forward and Up because target applications differ. |
| Maya | Y-up by default, but Y/Z up and linear units are per-scene settings; default linear unit is centimeter. |
| Houdini | Right-handed, Y-up, meters in the Houdini Engine coordinate guidance; export settings still matter. |
| 3ds Max | Right-handed, +Z up; system units and display units are separate, with the default system unit equal to one inch. |
| Unreal Engine | Left-handed, +X forward, +Y right, +Z up; centimeters. |
| Unity | Left-handed, +X right, +Y up, +Z forward; physics convention treats one world unit as one meter. |
| Godot | Right-handed, +X right, +Y up, -Z forward for built-in types such as Camera3D. |
| OpenUSD | Right-handed geometry; stage-wide `upAxis` and `metersPerUnit` metadata must be inspected. Cameras view down -Z. |
| FBX SDK | Scene axis and system-unit conversion APIs can rewrite node transforms and animation; SDK-internal objects use a right-handed Y-up, centimeter convention. |

## Official sources

- [3D Transform Visualizer](https://aike.github.io/3dtr/) and its [MIT-licensed source](https://github.com/aike/3dtr)
- [Khronos glTF 2.0 coordinate system and units](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#coordinate-system-and-units)
- [Blender FBX import/export axis settings](https://docs.blender.org/manual/en/3.0/addons/import_export/scene_fbx.html)
- [Maya axis orientation](https://help.autodesk.com/view/MAYAUL/2024/ENU/?guid=GUID-FDC58F4E-63B9-4012-B232-5F2FBAC5EAC9) and [default scene units](https://help.autodesk.com/cloudhelp/2023/ENU/Maya-Customizing/files/GUID-4D653DC9-57AA-4D8B-987A-5B7A9735CAF0.htm)
- [Houdini Engine coordinate systems](https://www.sidefx.com/docs/houdini/unreal/coordinates.html)
- [3ds Max coordinate system](https://help.autodesk.com/cloudhelp/2020/ENU/Max-Developer-Help/files/developer/3ds_max_sdk_features/3dxi/3dxi_initialization.html) and [system units](https://help.autodesk.com/cloudhelp/2024/ENU/3DSMax-Reference/files/GUID-BDE3C0F2-B27E-4C18-87C2-93E68996F74C.htm)
- [Unreal Engine coordinate system](https://dev.epicgames.com/documentation/en-us/unreal-engine/coordinate-system-and-spaces-in-unreal-engine) and [Maya import unit guidance](https://dev.epicgames.com/documentation/en-us/unreal-engine/importing-content-into-unreal-engine-from-maya)
- [Unity rotation and coordinate system](https://docs.unity3d.com/2023.2/Documentation/Manual/QuaternionAndEulerRotationsInUnity.html) and [Transform scale guidance](https://docs.unity3d.com/2022.1/Documentation/Manual/class-Transform.html)
- [Godot Basis coordinate conventions](https://docs.godotengine.org/en/stable/classes/class_basis.html)
- [OpenUSD geometry conventions](https://openusd.org/release/api/usd_geom_page_front.html) and [`metersPerUnit`](https://openusd.org/release/api/group___usd_geom_linear_units__group.html)
- [Autodesk FBX scene axis and unit conversion](https://help.autodesk.com/cloudhelp/2020/ENU/FBX-Developer-Help/files/nodes_and_scene_graph/fbx_scenes/FBX_Developer_Help_nodes_and_scene_graph_fbx_scenes_scene_axis_and_unit_conversion_html.html)

## Acceptance checks

Before accepting a transfer, compare at least one non-symmetric landmark and:

- world-space bounds and physical size;
- front/up orientation and camera view direction;
- triangle winding, normals, and tangent handedness;
- parented transforms and non-uniform scale behavior;
- animation start/end poses and rotation interpolation.
