pub(super) fn pip_install_args(package_spec: &str) -> Vec<String> {
    vec![
        "-m".into(),
        "pip".into(),
        "install".into(),
        "--upgrade".into(),
        package_spec.into(),
    ]
}

pub(super) fn pip_uninstall_args(package: &str) -> Vec<String> {
    vec![
        "-m".into(),
        "pip".into(),
        "uninstall".into(),
        "-y".into(),
        package.to_string(),
    ]
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PipArtifact {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub path: std::path::PathBuf,
    pub metadata_path: std::path::PathBuf,
    pub metadata: String,
}

pub(super) fn pip_restore_args_for_artifact(package: &str, artifact: &PipArtifact) -> Vec<String> {
    let path = if artifact.path.is_absolute() {
        artifact.path.clone()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(&artifact.path))
            .unwrap_or_else(|_| artifact.path.clone())
    };
    let path = path.to_string_lossy();
    let path = if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.into_owned()
    };
    let prefix = if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    };
    let encoded = path
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'.' | b'_' | b'/' | b':' | b'~')
            {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect::<String>();
    let file_url = format!("{prefix}{encoded}");
    pip_install_args(&format!(
        "{package} @ {file_url}#sha256={}",
        artifact.sha256
    ))
}

impl Drop for PipArtifact {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub(super) fn pip_show_args(package: &str) -> Vec<String> {
    vec![
        "-m".into(),
        "pip".into(),
        "show".into(),
        package.to_string(),
    ]
}

pub(super) fn pip_show_version(output: &[u8]) -> Option<String> {
    String::from_utf8_lossy(output)
        .lines()
        .find_map(|line| line.strip_prefix("Version:").map(str::trim))
        .filter(|version| !version.is_empty() && !version.chars().any(char::is_whitespace))
        .map(str::to_owned)
}
