//! Python-facing data structs for 3D scene geometry and animation types.
//!
//! Exports `PyObjectTransform`, `PyBoundingBox`, `PySceneObject`,
//! `PyFrameRange`, and `PyRenderOutput`.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pyclass;

#[cfg(feature = "python-bindings")]
use std::collections::HashMap;

#[cfg(feature = "python-bindings")]
use crate::adapters::{BoundingBox, FrameRange, ObjectTransform, RenderOutput, SceneObject};

// ── PyObjectTransform ──

/// Python-facing 3D object transform (TRS).
///
/// ```python
/// from dcc_mcp_core import ObjectTransform
///
/// t = ObjectTransform(translate=[0.0, 10.0, 0.0], rotate=[0.0, 45.0, 0.0], scale=[1.0, 1.0, 1.0])
/// print(t.translate)  # [0.0, 10.0, 0.0]
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "ObjectTransform", get_all, set_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyObjectTransform {
    /// World-space translation [x, y, z] in centimeters.
    pub translate: [f64; 3],
    /// Euler XYZ rotation in degrees [rx, ry, rz].
    pub rotate: [f64; 3],
    /// Scale [sx, sy, sz].
    pub scale: [f64; 3],
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyObjectTransform {
    #[new]
    #[pyo3(signature = (translate=None, rotate=None, scale=None))]
    fn new(translate: Option<[f64; 3]>, rotate: Option<[f64; 3]>, scale: Option<[f64; 3]>) -> Self {
        Self {
            translate: translate.unwrap_or([0.0, 0.0, 0.0]),
            rotate: rotate.unwrap_or([0.0, 0.0, 0.0]),
            scale: scale.unwrap_or([1.0, 1.0, 1.0]),
        }
    }

    /// Create an identity transform (no translation/rotation, unit scale).
    #[staticmethod]
    fn identity() -> Self {
        Self {
            translate: [0.0, 0.0, 0.0],
            rotate: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("translate", self.translate.to_vec())?;
        dict.set_item("rotate", self.rotate.to_vec())?;
        dict.set_item("scale", self.scale.to_vec())?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "ObjectTransform(translate={:?}, rotate={:?}, scale={:?})",
            self.translate, self.rotate, self.scale
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&ObjectTransform> for PyObjectTransform {
    fn from(t: &ObjectTransform) -> Self {
        Self {
            translate: t.translate,
            rotate: t.rotate,
            scale: t.scale,
        }
    }
}

#[cfg(feature = "python-bindings")]
impl From<&PyObjectTransform> for ObjectTransform {
    fn from(t: &PyObjectTransform) -> Self {
        Self {
            translate: t.translate,
            rotate: t.rotate,
            scale: t.scale,
        }
    }
}

// ── PyBoundingBox ──

/// Python-facing axis-aligned bounding box.
///
/// ```python
/// from dcc_mcp_core import BoundingBox
///
/// bb = BoundingBox(min=[-1.0, 0.0, -1.0], max=[1.0, 2.0, 1.0])
/// print(bb.center())  # [0.0, 1.0, 0.0]
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "BoundingBox", get_all, set_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyBoundingBox {
    /// Minimum corner [x, y, z] in world space (centimeters).
    pub min: [f64; 3],
    /// Maximum corner [x, y, z] in world space (centimeters).
    pub max: [f64; 3],
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyBoundingBox {
    #[new]
    #[pyo3(signature = (min=None, max=None))]
    fn new(min: Option<[f64; 3]>, max: Option<[f64; 3]>) -> Self {
        Self {
            min: min.unwrap_or([0.0, 0.0, 0.0]),
            max: max.unwrap_or([0.0, 0.0, 0.0]),
        }
    }

    /// Compute the center of the bounding box.
    fn center(&self) -> [f64; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    /// Compute the size (extents) [width, height, depth].
    fn size(&self) -> [f64; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("min", self.min.to_vec())?;
        dict.set_item("max", self.max.to_vec())?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!("BoundingBox(min={:?}, max={:?})", self.min, self.max)
    }
}

#[cfg(feature = "python-bindings")]
impl From<&BoundingBox> for PyBoundingBox {
    fn from(bb: &BoundingBox) -> Self {
        Self {
            min: bb.min,
            max: bb.max,
        }
    }
}

// ── PySceneObject ──

/// Python-facing lightweight scene object descriptor.
///
/// ```python
/// from dcc_mcp_core import SceneObject
///
/// obj = SceneObject(name="pCube1", long_name="|group1|pCube1", object_type="mesh")
/// print(obj.name)  # "pCube1"
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "SceneObject", get_all, set_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PySceneObject {
    pub name: String,
    pub long_name: String,
    pub object_type: String,
    pub parent: Option<String>,
    pub visible: bool,
    pub metadata: HashMap<String, String>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PySceneObject {
    #[new]
    #[pyo3(signature = (name, long_name=None, object_type="transform".to_string(), parent=None, visible=true, metadata=None))]
    fn new(
        name: String,
        long_name: Option<String>,
        object_type: String,
        parent: Option<String>,
        visible: bool,
        metadata: Option<HashMap<String, String>>,
    ) -> Self {
        let long_name = long_name.unwrap_or_else(|| name.clone());
        Self {
            name,
            long_name,
            object_type,
            parent,
            visible,
            metadata: metadata.unwrap_or_default(),
        }
    }

    pub fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("name", &self.name)?;
        dict.set_item("long_name", &self.long_name)?;
        dict.set_item("object_type", &self.object_type)?;
        dict.set_item("parent", &self.parent)?;
        dict.set_item("visible", self.visible)?;
        dict.set_item("metadata", &self.metadata)?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "SceneObject(name={:?}, type={:?}, visible={})",
            self.name, self.object_type, self.visible
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&SceneObject> for PySceneObject {
    fn from(obj: &SceneObject) -> Self {
        Self {
            name: obj.name.clone(),
            long_name: obj.long_name.clone(),
            object_type: obj.object_type.clone(),
            parent: obj.parent.clone(),
            visible: obj.visible,
            metadata: obj.metadata.clone(),
        }
    }
}

// ── PyFrameRange ──

/// Python-facing frame range and timing information.
///
/// ```python
/// from dcc_mcp_core import FrameRange
///
/// fr = FrameRange(start=1.0, end=240.0, fps=24.0, current=1.0)
/// print(fr.end - fr.start)  # 239.0
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "FrameRange", get_all, set_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyFrameRange {
    pub start: f64,
    pub end: f64,
    pub fps: f64,
    pub current: f64,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyFrameRange {
    #[new]
    #[pyo3(signature = (start=1.0, end=100.0, fps=24.0, current=1.0))]
    fn new(start: f64, end: f64, fps: f64, current: f64) -> Self {
        Self {
            start,
            end,
            fps,
            current,
        }
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("start", self.start)?;
        dict.set_item("end", self.end)?;
        dict.set_item("fps", self.fps)?;
        dict.set_item("current", self.current)?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "FrameRange(start={}, end={}, fps={}, current={})",
            self.start, self.end, self.fps, self.current
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&FrameRange> for PyFrameRange {
    fn from(fr: &FrameRange) -> Self {
        Self {
            start: fr.start,
            end: fr.end,
            fps: fr.fps,
            current: fr.current,
        }
    }
}

// ── PyRenderOutput ──

/// Python-facing render output metadata.
///
/// ```python
/// from dcc_mcp_core import RenderOutput
///
/// out = RenderOutput(file_path="/renders/frame001.png", width=1920, height=1080,
///                    format="png", render_time_ms=5000)
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "RenderOutput", get_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyRenderOutput {
    pub file_path: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub render_time_ms: u64,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyRenderOutput {
    #[new]
    #[pyo3(signature = (file_path, width, height, format, render_time_ms=0))]
    fn new(
        file_path: String,
        width: u32,
        height: u32,
        format: String,
        render_time_ms: u64,
    ) -> Self {
        Self {
            file_path,
            width,
            height,
            format,
            render_time_ms,
        }
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("file_path", &self.file_path)?;
        dict.set_item("width", self.width)?;
        dict.set_item("height", self.height)?;
        dict.set_item("format", &self.format)?;
        dict.set_item("render_time_ms", self.render_time_ms)?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "RenderOutput({}x{}, format={:?}, time={}ms)",
            self.width, self.height, self.format, self.render_time_ms
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&RenderOutput> for PyRenderOutput {
    fn from(out: &RenderOutput) -> Self {
        Self {
            file_path: out.file_path.clone(),
            width: out.width,
            height: out.height,
            format: out.format.clone(),
            render_time_ms: out.render_time_ms,
        }
    }
}
