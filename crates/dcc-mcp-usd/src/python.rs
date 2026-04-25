//! PyO3 Python bindings for `dcc-mcp-usd`.
//!
//! Exposes `PyUsdStage`, `PyUsdPrim`, `PyUsdLayer`, `PySdfPath`, `PyVtValue`
//! to Python as `dcc_mcp_core._core` classes.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};

use crate::bridge::{
    meters_per_unit_to_units, scene_info_to_stage, stage_to_scene_info, units_to_meters_per_unit,
};
use crate::stage::UsdStage;
use crate::types::{SdfPath, UsdAttribute, UsdPrim, VtValue};

// ── PySdfPath ─────────────────────────────────────────────────────────────────

/// A USD scene description path (e.g. ``/World/Cube``).
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "SdfPath", from_py_object)]
#[derive(Clone)]
pub struct PySdfPath {
    inner: SdfPath,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PySdfPath {
    #[new]
    pub fn new(path: &str) -> PyResult<Self> {
        SdfPath::new(path)
            .map(|inner| Self { inner })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// String representation of this path.
    pub fn __str__(&self) -> &str {
        self.inner.as_str()
    }

    pub fn __repr__(&self) -> String {
        format!("SdfPath('{}')", self.inner.as_str())
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }

    pub fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.inner.hash(&mut h);
        h.finish()
    }

    /// Append a child segment and return a new path.
    pub fn child(&self, name: &str) -> PyResult<Self> {
        self.inner
            .child(name)
            .map(|inner| Self { inner })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Parent path, or ``None`` for the root path.
    pub fn parent(&self) -> Option<Self> {
        self.inner.parent().map(|inner| Self { inner })
    }

    /// Whether this is an absolute path.
    #[getter]
    pub fn is_absolute(&self) -> bool {
        self.inner.is_absolute()
    }

    /// Last path element name (e.g. ``"Cube"`` for ``/World/Cube``).
    #[getter]
    pub fn name(&self) -> &str {
        self.inner.name()
    }
}

// ── PyVtValue ─────────────────────────────────────────────────────────────────

/// A USD variant value (bool, int, float, string, vec3f, etc.).
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "VtValue", from_py_object)]
#[derive(Clone)]
pub struct PyVtValue {
    pub inner: VtValue,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyVtValue {
    /// The USD type name string (e.g. ``"float3"``, ``"token"``).
    #[getter]
    pub fn type_name(&self) -> &str {
        self.inner.type_name()
    }

    pub fn __repr__(&self) -> String {
        format!("VtValue({}: {:?})", self.inner.type_name(), self.inner)
    }

    /// Create a bool value.
    #[staticmethod]
    pub fn from_bool(v: bool) -> Self {
        Self {
            inner: VtValue::Bool(v),
        }
    }

    /// Create an int value.
    #[staticmethod]
    pub fn from_int(v: i32) -> Self {
        Self {
            inner: VtValue::Int(v),
        }
    }

    /// Create a float value.
    #[staticmethod]
    pub fn from_float(v: f32) -> Self {
        Self {
            inner: VtValue::Float(v),
        }
    }

    /// Create a string value.
    #[staticmethod]
    pub fn from_string(v: String) -> Self {
        Self {
            inner: VtValue::String(v),
        }
    }

    /// Create a token value (USD enum identifier).
    #[staticmethod]
    pub fn from_token(v: String) -> Self {
        Self {
            inner: VtValue::Token(v),
        }
    }

    /// Create an asset path value.
    #[staticmethod]
    pub fn from_asset(v: String) -> Self {
        Self {
            inner: VtValue::Asset(v),
        }
    }

    /// Create a 3D float vector.
    #[staticmethod]
    pub fn from_vec3f(x: f32, y: f32, z: f32) -> Self {
        Self {
            inner: VtValue::Vec3f(x, y, z),
        }
    }

    /// Convert to a Python primitive.  Returns ``None`` for array/matrix types.
    pub fn to_python(&self, py: Python<'_>) -> Py<PyAny> {
        match &self.inner {
            VtValue::Bool(v) => v.into_pyobject(py).unwrap().to_owned().into_any().unbind(),
            VtValue::Int(v) => v.into_pyobject(py).unwrap().into_any().unbind(),
            VtValue::Int64(v) => v.into_pyobject(py).unwrap().into_any().unbind(),
            VtValue::Float(v) => v.into_pyobject(py).unwrap().into_any().unbind(),
            VtValue::Double(v) => v.into_pyobject(py).unwrap().into_any().unbind(),
            VtValue::String(s) | VtValue::Token(s) | VtValue::Asset(s) => {
                s.into_pyobject(py).unwrap().into_any().unbind()
            }
            VtValue::Vec3f(x, y, z) => (*x, *y, *z).into_pyobject(py).unwrap().into_any().unbind(),
            VtValue::Vec2f(x, y) => (*x, *y).into_pyobject(py).unwrap().into_any().unbind(),
            VtValue::Vec4f(x, y, z, w) => (*x, *y, *z, *w)
                .into_pyobject(py)
                .unwrap()
                .into_any()
                .unbind(),
            VtValue::FloatArray(arr) => arr.into_pyobject(py).unwrap().into_any().unbind(),
            VtValue::IntArray(arr) => arr.into_pyobject(py).unwrap().into_any().unbind(),
            VtValue::StringArray(arr) => arr.into_pyobject(py).unwrap().into_any().unbind(),
            _ => py.None(),
        }
    }
}

// ── PyUsdPrim ─────────────────────────────────────────────────────────────────

/// A prim (primitive) within a USD stage.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "UsdPrim", from_py_object)]
#[derive(Clone)]
pub struct PyUsdPrim {
    pub inner: UsdPrim,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyUsdPrim {
    pub fn __repr__(&self) -> String {
        format!(
            "UsdPrim('{}', type='{}')",
            self.inner.path, self.inner.type_name
        )
    }

    /// Absolute path of this prim.
    #[getter]
    pub fn path(&self) -> PySdfPath {
        PySdfPath {
            inner: self.inner.path.clone(),
        }
    }

    /// USD type name (e.g. ``"Mesh"``, ``"Camera"``).
    #[getter]
    pub fn type_name(&self) -> &str {
        &self.inner.type_name
    }

    /// Whether this prim is active.
    #[getter]
    pub fn active(&self) -> bool {
        self.inner.active
    }

    /// Last path element name.
    #[getter]
    pub fn name(&self) -> &str {
        self.inner.name()
    }

    /// Set or update an attribute value.
    pub fn set_attribute(&mut self, name: &str, value: &PyVtValue) {
        self.inner
            .attributes
            .entry(name.to_string())
            .and_modify(|a| a.default_value = Some(value.inner.clone()))
            .or_insert_with(|| UsdAttribute::new(name, value.inner.clone()));
    }

    /// Get an attribute value.  Returns ``None`` if not found.
    pub fn get_attribute(&self, name: &str) -> Option<PyVtValue> {
        self.inner.get_attribute(name).and_then(|a| {
            a.default_value
                .as_ref()
                .map(|v| PyVtValue { inner: v.clone() })
        })
    }

    /// List all attribute names.
    pub fn attribute_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.inner.attributes.keys().cloned().collect();
        names.sort();
        names
    }

    /// Return a dict of ``{attr_name: type_name}``.
    pub fn attributes_summary<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        for (name, attr) in &self.inner.attributes {
            let type_name = attr
                .default_value
                .as_ref()
                .map(|v| v.type_name())
                .unwrap_or("none");
            d.set_item(name, type_name)?;
        }
        Ok(d)
    }

    /// Whether the given API schema is applied to this prim.
    pub fn has_api(&self, schema: &str) -> bool {
        self.inner.has_api(schema)
    }
}

// ── PyUsdStage ────────────────────────────────────────────────────────────────

/// A composed USD stage — the primary unit of scene exchange.
///
/// Example::
///
///     from dcc_mcp_core import UsdStage, SdfPath, VtValue
///
///     stage = UsdStage("my_scene")
///     prim = stage.define_prim("/World/Cube", "Mesh")
///     stage.set_attribute("/World/Cube", "radius", VtValue.from_float(1.0))
///     print(stage.export_usda())
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "UsdStage")]
pub struct PyUsdStage {
    pub inner: UsdStage,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyUsdStage {
    /// Create a new empty stage with the given name.
    #[new]
    pub fn new(name: &str) -> Self {
        Self {
            inner: UsdStage::new(name),
        }
    }

    pub fn __repr__(&self) -> String {
        format!(
            "UsdStage(name='{}', prims={})",
            self.inner.name,
            self.inner.root_layer.prims.len()
        )
    }

    /// Stage name.
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Unique stage ID (UUID string).
    #[getter]
    pub fn id(&self) -> &str {
        &self.inner.id
    }

    /// Default prim path hint.
    #[getter]
    pub fn default_prim(&self) -> Option<&str> {
        self.inner.default_prim.as_deref()
    }

    #[setter]
    pub fn set_default_prim_prop(&mut self, value: Option<String>) {
        self.inner.default_prim = value;
    }

    /// Set the default prim path (callable method form).
    pub fn set_default_prim(&mut self, path: &str) {
        self.inner.default_prim = Some(path.to_string());
    }

    // ── Prim operations ──

    /// Define a prim at the given path with the given USD type name.
    ///
    /// Returns the newly created (or replaced) prim.
    pub fn define_prim(&mut self, path: &str, type_name: &str) -> PyResult<PyUsdPrim> {
        let sdf = SdfPath::new(path).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let prim = self.inner.define_prim(sdf, type_name);
        Ok(PyUsdPrim {
            inner: prim.clone(),
        })
    }

    /// Get a prim by path.  Returns ``None`` if not found.
    pub fn get_prim(&self, path: &str) -> Option<PyUsdPrim> {
        self.inner
            .get_prim(path)
            .map(|p| PyUsdPrim { inner: p.clone() })
    }

    /// Whether the stage has a prim at the given path.
    pub fn has_prim(&self, path: &str) -> bool {
        self.inner.has_prim(path)
    }

    /// Remove a prim from the root layer.  Returns ``True`` if removed.
    pub fn remove_prim(&mut self, path: &str) -> bool {
        self.inner.remove_prim(path)
    }

    /// Return the number of prims in the root layer.
    pub fn prim_count(&self) -> usize {
        self.inner.root_layer.prims.len()
    }

    /// Return all prims as a list of ``UsdPrim`` objects (alias for `traverse`).
    pub fn list_prims(&self) -> Vec<PyUsdPrim> {
        self.inner
            .traverse()
            .into_iter()
            .map(|p| PyUsdPrim { inner: p.clone() })
            .collect()
    }

    /// Return all prims as a list of ``UsdPrim`` objects.
    pub fn traverse(&self) -> Vec<PyUsdPrim> {
        self.inner
            .traverse()
            .into_iter()
            .map(|p| PyUsdPrim { inner: p.clone() })
            .collect()
    }

    /// Return all prims of the given USD type.
    pub fn prims_of_type(&self, type_name: &str) -> Vec<PyUsdPrim> {
        self.inner
            .prims_of_type(type_name)
            .into_iter()
            .map(|p| PyUsdPrim { inner: p.clone() })
            .collect()
    }

    // ── Attribute operations ──

    /// Set an attribute on a prim.
    pub fn set_attribute(
        &mut self,
        prim_path: &str,
        attr_name: &str,
        value: &PyVtValue,
    ) -> PyResult<()> {
        self.inner
            .set_attribute(prim_path, attr_name, value.inner.clone())
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Get an attribute value from a prim.  Returns ``None`` if prim or attr not found.
    pub fn get_attribute(&self, prim_path: &str, attr_name: &str) -> PyResult<Option<PyVtValue>> {
        self.inner
            .get_attribute(prim_path, attr_name)
            .map(|opt| opt.map(|v| PyVtValue { inner: v.clone() }))
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    // ── Metrics ──

    /// Return stage metrics as a dictionary.
    pub fn metrics<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let m = self.inner.metrics();
        let d = PyDict::new(py);
        d.set_item("prim_count", m.prim_count)?;
        d.set_item("mesh_count", m.mesh_count)?;
        d.set_item("camera_count", m.camera_count)?;
        d.set_item("light_count", m.light_count)?;
        d.set_item("material_count", m.material_count)?;
        d.set_item("xform_count", m.xform_count)?;
        Ok(d)
    }

    // ── Serialization ──

    /// Serialize the stage to a JSON string.
    pub fn to_json(&self) -> PyResult<String> {
        self.inner
            .to_json()
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Deserialize a stage from a JSON string.
    #[staticmethod]
    pub fn from_json(json: &str) -> PyResult<Self> {
        UsdStage::from_json(json)
            .map(|inner| Self { inner })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Export the stage as USDA (USD ASCII) text.
    pub fn export_usda(&self) -> String {
        self.inner.export_usda()
    }

    // ── Layer metadata ──

    /// Up axis of the root layer (``"Y"`` or ``"Z"``).
    #[getter]
    pub fn up_axis(&self) -> &str {
        &self.inner.root_layer.up_axis
    }

    #[setter]
    pub fn set_up_axis(&mut self, axis: &str) {
        self.inner.root_layer.up_axis = axis.to_uppercase();
    }

    /// Meters per unit of the root layer.
    #[getter]
    pub fn meters_per_unit(&self) -> f64 {
        self.inner.root_layer.meters_per_unit
    }

    #[setter]
    pub fn set_meters_per_unit_prop(&mut self, mpu: f64) {
        self.inner.root_layer.meters_per_unit = mpu;
    }

    /// Set the meters per unit value (callable method form).
    pub fn set_meters_per_unit(&mut self, mpu: f64) {
        self.inner.root_layer.meters_per_unit = mpu;
    }

    /// Frames per second.
    #[getter]
    pub fn fps(&self) -> Option<f64> {
        self.inner.root_layer.frames_per_second
    }

    #[setter]
    pub fn set_fps(&mut self, fps: Option<f64>) {
        self.inner.root_layer.frames_per_second = fps;
    }

    /// Start time code.
    #[getter]
    pub fn start_time_code(&self) -> Option<f64> {
        self.inner.root_layer.start_time_code
    }

    #[setter]
    pub fn set_start_time_code(&mut self, v: Option<f64>) {
        self.inner.root_layer.start_time_code = v;
    }

    /// End time code.
    #[getter]
    pub fn end_time_code(&self) -> Option<f64> {
        self.inner.root_layer.end_time_code
    }

    #[setter]
    pub fn set_end_time_code(&mut self, v: Option<f64>) {
        self.inner.root_layer.end_time_code = v;
    }
}

// ── Bridge Python functions ───────────────────────────────────────────────────

/// Convert a DCC ``SceneInfo`` dict to a ``UsdStage``.
///
/// Args:
///     scene_info_json: JSON string of a ``SceneInfo`` object.
///     dcc_type: The DCC type string (e.g. ``"maya"``). Defaults to ``"generic"``.
///
/// Returns:
///     A ``UsdStage`` containing the converted scene.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(signature = (scene_info_json, dcc_type = "generic"))]
pub fn scene_info_json_to_stage(scene_info_json: &str, dcc_type: &str) -> PyResult<PyUsdStage> {
    let info: dcc_mcp_protocols::adapters::SceneInfo = serde_json::from_str(scene_info_json)
        .map_err(|e| PyValueError::new_err(format!("invalid SceneInfo JSON: {e}")))?;
    let stage = scene_info_to_stage(&info, dcc_type);
    Ok(PyUsdStage { inner: stage })
}

/// Convert a ``UsdStage`` to a JSON string representing ``SceneInfo``.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
pub fn stage_to_scene_info_json(stage: &PyUsdStage) -> PyResult<String> {
    let info = stage_to_scene_info(&stage.inner);
    serde_json::to_string(&info).map_err(|e| PyValueError::new_err(e.to_string()))
}

/// Convert a unit string to meters per unit.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "units_to_mpu")]
pub fn py_units_to_mpu(units: &str) -> f64 {
    units_to_meters_per_unit(units)
}

/// Convert meters per unit to a unit string.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "mpu_to_units")]
pub fn py_mpu_to_units(mpu: f64) -> String {
    meters_per_unit_to_units(mpu)
}

// ── Registration ─────────────────────────────────────────────────────────────

/// Register all USD Python classes and functions on the module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySdfPath>()?;
    m.add_class::<PyVtValue>()?;
    m.add_class::<PyUsdPrim>()?;
    m.add_class::<PyUsdStage>()?;
    m.add_function(wrap_pyfunction!(scene_info_json_to_stage, m)?)?;
    m.add_function(wrap_pyfunction!(stage_to_scene_info_json, m)?)?;
    m.add_function(wrap_pyfunction!(py_units_to_mpu, m)?)?;
    m.add_function(wrap_pyfunction!(py_mpu_to_units, m)?)?;
    Ok(())
}
