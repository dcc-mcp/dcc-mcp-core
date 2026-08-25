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

pub(super) fn pip_show_args(package: &str) -> Vec<String> {
    vec![
        "-m".into(),
        "pip".into(),
        "show".into(),
        package.to_string(),
    ]
}
