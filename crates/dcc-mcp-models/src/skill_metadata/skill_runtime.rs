#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pyclass_enum};
use serde::{Deserialize, Serialize};

/// Declarative optional runtime dependency category for adapter skills.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass_enum)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillRuntimeKind", eq, eq_int, from_py_object)
)]
#[serde(rename_all = "snake_case")]
pub enum SkillRuntimeKind {
    /// Python package or wheel dependency, commonly checked by module name.
    #[default]
    PythonPackage,
    /// Python optional extra such as `dcc-mcp-openusd[usd-core]`.
    PythonExtra,
    /// Command-line binary expected on `PATH`.
    Binary,
    /// Environment variable expected to be non-empty.
    EnvVar,
    /// Declarative feature level with no automatic probe.
    Feature,
}

/// Resolved optional runtime state surfaced through discovery APIs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass_enum)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillRuntimeState", eq, eq_int, from_py_object)
)]
#[serde(rename_all = "snake_case")]
pub enum SkillRuntimeState {
    /// All declared runtime checks are satisfied.
    Available,
    /// Optional runtime is absent; skill can still run with reduced capability.
    #[default]
    Degraded,
    /// Required runtime is absent; the advertised capability is not callable.
    Missing,
}

/// A single optional runtime descriptor from `metadata.dcc-mcp.runtimes`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillRuntimeDescriptor", get_all, from_py_object)
)]
pub struct SkillRuntimeDescriptor {
    /// Stable runtime identifier, for example `usd-core` or `usdcat`.
    #[serde(default)]
    pub name: String,

    /// Runtime category.
    #[serde(default, rename = "type")]
    pub kind: SkillRuntimeKind,

    /// Human-readable explanation of what this runtime unlocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Optional feature level exposed when this runtime is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_level: Option<String>,

    /// Python package name, such as `usd-core`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,

    /// Python module name to check with `importlib.util.find_spec`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,

    /// Python extra name, such as `usd-core`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<String>,

    /// Binary name to resolve on `PATH`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,

    /// Environment variable name to check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,

    /// Whether the skill can still run when this runtime is absent.
    #[serde(default, alias = "optional")]
    pub optional: bool,

    /// Human-actionable install or configuration guidance.
    #[serde(
        default,
        alias = "install_hint",
        alias = "install-hint",
        skip_serializing_if = "Option::is_none"
    )]
    pub guidance: Option<String>,
}

impl SkillRuntimeDescriptor {
    /// Resolve a descriptor without importing or executing the skill's tool script.
    pub fn resolve(&self) -> SkillRuntimeReport {
        let state = match self.kind {
            SkillRuntimeKind::EnvVar => self.resolve_env_var(),
            SkillRuntimeKind::Binary => self.resolve_binary(),
            SkillRuntimeKind::PythonPackage | SkillRuntimeKind::PythonExtra => {
                self.resolve_python_runtime()
            }
            SkillRuntimeKind::Feature => self.default_absent_state(),
        };
        SkillRuntimeReport {
            name: self.display_name(),
            kind: self.kind,
            state,
            description: self.description.clone(),
            feature_level: self.feature_level.clone(),
            package: self.package.clone(),
            module: self.module.clone(),
            extra: self.extra.clone(),
            binary: self.binary.clone(),
            env: self.env.clone(),
            guidance: self.guidance.clone(),
        }
    }

    fn display_name(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        self.package
            .as_ref()
            .or(self.extra.as_ref())
            .or(self.module.as_ref())
            .or(self.binary.as_ref())
            .or(self.env.as_ref())
            .cloned()
            .unwrap_or_else(|| "runtime".to_string())
    }

    fn default_absent_state(&self) -> SkillRuntimeState {
        if self.optional {
            SkillRuntimeState::Degraded
        } else {
            SkillRuntimeState::Missing
        }
    }

    fn resolve_env_var(&self) -> SkillRuntimeState {
        let fallback = (!self.name.is_empty()).then_some(&self.name);
        let Some(name) = self.env.as_ref().or(fallback) else {
            return self.default_absent_state();
        };
        match std::env::var_os(name) {
            Some(value) if !value.is_empty() => SkillRuntimeState::Available,
            _ => self.default_absent_state(),
        }
    }

    fn resolve_binary(&self) -> SkillRuntimeState {
        let fallback = (!self.name.is_empty()).then_some(&self.name);
        let Some(binary) = self.binary.as_ref().or(fallback) else {
            return self.default_absent_state();
        };
        if binary_on_path(binary) {
            SkillRuntimeState::Available
        } else {
            self.default_absent_state()
        }
    }

    fn resolve_python_runtime(&self) -> SkillRuntimeState {
        let Some(module) = self.module.as_deref() else {
            return self.default_absent_state();
        };
        match python_module_available(module) {
            Some(true) => SkillRuntimeState::Available,
            Some(false) => self.default_absent_state(),
            None => self.default_absent_state(),
        }
    }
}

/// Resolved runtime row exposed by search/list/describe surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillRuntimeReport", get_all, from_py_object)
)]
pub struct SkillRuntimeReport {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: SkillRuntimeKind,
    pub state: SkillRuntimeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

/// Aggregated runtime status for compact discovery.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillRuntimeSummary", get_all, from_py_object)
)]
pub struct SkillRuntimeSummary {
    pub state: SkillRuntimeState,
    pub available: usize,
    pub degraded: usize,
    pub missing: usize,
    pub total: usize,
}

pub fn resolve_runtime_reports(runtimes: &[SkillRuntimeDescriptor]) -> Vec<SkillRuntimeReport> {
    runtimes
        .iter()
        .map(SkillRuntimeDescriptor::resolve)
        .collect()
}

pub fn summarize_runtime_reports(reports: &[SkillRuntimeReport]) -> SkillRuntimeSummary {
    let mut summary = SkillRuntimeSummary {
        total: reports.len(),
        ..Default::default()
    };
    for report in reports {
        match report.state {
            SkillRuntimeState::Available => summary.available += 1,
            SkillRuntimeState::Degraded => summary.degraded += 1,
            SkillRuntimeState::Missing => summary.missing += 1,
        }
    }
    summary.state = if summary.missing > 0 {
        SkillRuntimeState::Missing
    } else if summary.degraded > 0 {
        SkillRuntimeState::Degraded
    } else {
        SkillRuntimeState::Available
    };
    summary
}

fn binary_on_path(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let candidates = binary_candidates(binary);
    std::env::split_paths(&paths).any(|dir| candidates.iter().any(|name| dir.join(name).is_file()))
}

fn binary_candidates(binary: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        if std::path::Path::new(binary).extension().is_some() {
            return vec![binary.to_string()];
        }
        let pathext =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        let mut out = vec![binary.to_string()];
        out.extend(
            pathext
                .split(';')
                .filter(|ext| !ext.is_empty())
                .map(|ext| format!("{binary}{ext}")),
        );
        out
    }
    #[cfg(not(windows))]
    {
        vec![binary.to_string()]
    }
}

#[cfg(feature = "python-bindings")]
fn python_module_available(module: &str) -> Option<bool> {
    use pyo3::prelude::*;
    Python::try_attach(|py| {
        let importlib = py.import("importlib.util").ok()?;
        let spec = importlib.call_method1("find_spec", (module,)).ok()?;
        Some(!spec.is_none())
    })
    .and_then(|result| result)
}

#[cfg(not(feature = "python-bindings"))]
fn python_module_available(_module: &str) -> Option<bool> {
    None
}
