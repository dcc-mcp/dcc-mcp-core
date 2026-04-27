//! PyO3 bindings for `SkillScope`.

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;

use crate::skill_scope::SkillScope;

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl SkillScope {
    fn __repr__(&self) -> String {
        format!("SkillScope.{}", self.label())
    }

    fn __str__(&self) -> String {
        self.label().to_string()
    }
}
