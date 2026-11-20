use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dcc_mcp_skills::gui_executable::{GuiExecutableHint, is_gui_executable};

/// Environment variable consulted when resolving an install interpreter.
pub const INSTALL_PYTHON_ENV: &str = "DCC_MCP_INSTALL_PYTHON";

/// Where a selected pip interpreter came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PythonSource {
    /// `--python` on the install request.
    Requested,
    /// `DCC_MCP_INSTALL_PYTHON` in the injected environment.
    Environment,
    /// Catalog adapter metadata (`python_path`).
    Catalog,
    /// Embedded interpreter discovered next to the selected DCC executable.
    DiscoveredHost,
    /// No interpreter was required for this install type.
    NotRequired,
}

impl PythonSource {
    /// Stable machine-readable label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Environment => "environment",
            Self::Catalog => "catalog",
            Self::DiscoveredHost => "discovered_host",
            Self::NotRequired => "not_required",
        }
    }
}

/// Injectable process environment. Tests provide only the variables they need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallEnv {
    vars: BTreeMap<String, String>,
}

impl InstallEnv {
    /// Capture `INSTALL_PYTHON_ENV` from the current process.
    #[must_use]
    pub fn from_process() -> Self {
        let mut vars = BTreeMap::new();
        if let Some(value) = std::env::var_os(INSTALL_PYTHON_ENV) {
            vars.insert(
                INSTALL_PYTHON_ENV.to_string(),
                value.to_string_lossy().into_owned(),
            );
        }
        Self { vars }
    }

    /// Build an environment view from explicit variables.
    #[must_use]
    pub fn from_vars(vars: BTreeMap<String, String>) -> Self {
        Self { vars }
    }

    /// Read the install Python override.
    #[must_use]
    pub fn install_python(&self) -> Option<&str> {
        self.vars
            .get(INSTALL_PYTHON_ENV)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }
}

/// The selected interpreter and the source that resolved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPython {
    /// Interpreter path for the pip step.
    pub path: String,
    /// Resolution precedence that produced `path`.
    pub source: PythonSource,
}

/// Why an install plan could not resolve a safe interpreter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonResolutionError {
    /// A configured or discovered path names a known DCC GUI binary.
    GuiExecutable(GuiExecutableHint),
    /// A hosted adapter needs a safe interpreter but none could be resolved.
    MissingHostedInterpreter,
    /// More than one host installation supplied a usable embedded interpreter.
    AmbiguousHostedInterpreters { paths: Vec<PathBuf> },
}

/// One candidate host executable discovered without executing the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCandidate {
    /// Resolved DCC executable path.
    pub path: PathBuf,
    /// Whether the operator supplied this path explicitly.
    pub explicit: bool,
}

/// Select a pip interpreter without executing any host or probing PATH.
///
/// Precedence: explicit request, install environment, catalog, then a
/// discovered embedded interpreter. A hosted adapter with no safe interpreter
/// is an error; generic PATH `python` is never selected implicitly.
pub(crate) fn resolve_python(
    override_python: Option<String>,
    catalog_python: Option<String>,
    env: &InstallEnv,
    host_candidates: &[HostCandidate],
    dcc_type: &str,
) -> Result<ResolvedPython, PythonResolutionError> {
    let configured = override_python
        .filter(|path| !path.trim().is_empty())
        .map(|path| (path, PythonSource::Requested))
        .or_else(|| {
            env.install_python()
                .map(|path| (path.to_string(), PythonSource::Environment))
        })
        .or_else(|| {
            catalog_python
                .filter(|path| !path.trim().is_empty())
                .map(|path| (path, PythonSource::Catalog))
        });
    if let Some((path, source)) = configured {
        if let Some(hint) = is_gui_executable(Path::new(&path)) {
            return Err(PythonResolutionError::GuiExecutable(hint));
        }
        return Ok(ResolvedPython { path, source });
    }

    let host_key = normalized_host_family(dcc_type);
    let mut discovered = None;
    for candidate in host_candidates {
        let Some(hint) = is_gui_executable(&candidate.path) else {
            continue;
        };
        // Commandlet/GUI replacements are not generic `python -m pip` interpreters.
        if hint.dcc_kind != host_key || !is_supported_host_family(&host_key) {
            continue;
        }
        if candidate.explicit && !candidate.path.is_file() {
            return Err(PythonResolutionError::GuiExecutable(hint));
        }
        let Some(python) = hint
            .recommended_replacement
            .as_ref()
            .filter(|path| path.is_file())
            .and_then(|path| path.to_str())
            .map(str::to_owned)
        else {
            if candidate.explicit {
                return Err(PythonResolutionError::GuiExecutable(hint));
            }
            continue;
        };
        if candidate.explicit {
            return Ok(ResolvedPython {
                path: python,
                source: PythonSource::DiscoveredHost,
            });
        }
        if let Some(previous) = discovered {
            return Err(PythonResolutionError::AmbiguousHostedInterpreters {
                paths: vec![PathBuf::from(previous), PathBuf::from(python)],
            });
        }
        discovered = Some(python);
    }

    if let Some(path) = discovered {
        return Ok(ResolvedPython {
            path,
            source: PythonSource::DiscoveredHost,
        });
    }
    Err(PythonResolutionError::MissingHostedInterpreter)
}

/// Normalize a DCC type to the GUI detector's family names.
fn normalized_host_family(dcc_type: &str) -> String {
    match dcc_type.to_ascii_lowercase().as_str() {
        "maya" => "maya".to_string(),
        "houdini" => "houdini".to_string(),
        other => other.to_string(),
    }
}

fn is_supported_host_family(dcc_type: &str) -> bool {
    matches!(dcc_type, "maya" | "houdini")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_discovered_host_interpreters_are_rejected_as_ambiguous() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first_host = first_dir.path().join("maya.exe");
        let first_python = first_dir.path().join("mayapy.exe");
        let second_host = second_dir.path().join("maya.exe");
        let second_python = second_dir.path().join("mayapy.exe");
        for path in [&first_host, &first_python, &second_host, &second_python] {
            std::fs::write(path, b"stub").unwrap();
        }

        let error = resolve_python(
            None,
            None,
            &InstallEnv::default(),
            &[
                HostCandidate {
                    path: first_host,
                    explicit: false,
                },
                HostCandidate {
                    path: second_host,
                    explicit: false,
                },
            ],
            "maya",
        )
        .unwrap_err();

        assert!(matches!(
            error,
            PythonResolutionError::AmbiguousHostedInterpreters { paths }
                if paths == vec![first_python, second_python]
        ));
    }
}
