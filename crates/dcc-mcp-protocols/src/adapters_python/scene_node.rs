//! Python-facing scene hierarchy node for recursive tree traversal.
//!
//! Exports [`PySceneNode`].

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;

#[cfg(feature = "python-bindings")]
use crate::adapters::SceneNode;

#[cfg(feature = "python-bindings")]
use super::data::PySceneObject;

// ── PySceneNode ──

/// Python-facing scene hierarchy node (recursive tree).
///
/// ```python
/// from dcc_mcp_core import SceneNode, SceneObject
///
/// leaf = SceneNode(
///     object=SceneObject(name="pSphere1", object_type="mesh"),
///     children=[]
/// )
/// root = SceneNode(
///     object=SceneObject(name="group1", object_type="transform"),
///     children=[leaf]
/// )
/// print(len(root.children))  # 1
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "SceneNode", get_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PySceneNode {
    /// The scene object at this node.
    pub object: PySceneObject,
    /// Immediate children of this node.
    pub children: Vec<PySceneNode>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PySceneNode {
    #[new]
    #[pyo3(signature = (object, children=None))]
    fn new(object: PySceneObject, children: Option<Vec<PySceneNode>>) -> Self {
        Self {
            object,
            children: children.unwrap_or_default(),
        }
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("object", self.object.to_dict(py)?)?;
        let children: Vec<_> = self
            .children
            .iter()
            .map(|c| c.to_dict(py))
            .collect::<PyResult<_>>()?;
        dict.set_item("children", children)?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "SceneNode(name={:?}, children={})",
            self.object.name,
            self.children.len()
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&SceneNode> for PySceneNode {
    fn from(node: &SceneNode) -> Self {
        Self {
            object: PySceneObject::from(&node.object),
            children: node.children.iter().map(PySceneNode::from).collect(),
        }
    }
}
