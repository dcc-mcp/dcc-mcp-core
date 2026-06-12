#![cfg(all(test, feature = "admin"))]

use super::handlers::{AdminInstanceUpdateVersion, admin_instance_update_version};

#[test]
fn server_update_uses_gateway_package_version_when_request_omits_current_version() {
    match admin_instance_update_version("dcc-mcp-server", None) {
        AdminInstanceUpdateVersion::Known {
            current,
            display,
            source,
        } => {
            assert_eq!(current, env!("CARGO_PKG_VERSION"));
            assert_eq!(display.as_deref(), Some(env!("CARGO_PKG_VERSION")));
            assert_eq!(source, "gateway_package_version");
        }
        AdminInstanceUpdateVersion::MissingCurrentVersion => {
            panic!("server binary update must infer the gateway package version")
        }
    }
}

#[test]
fn server_update_prefers_explicit_binary_version_from_request() {
    match admin_instance_update_version("dcc-mcp-server", Some(" 0.18.0 ")) {
        AdminInstanceUpdateVersion::Known {
            current,
            display,
            source,
        } => {
            assert_eq!(current, "0.18.0");
            assert_eq!(display.as_deref(), Some("0.18.0"));
            assert_eq!(source, "request");
        }
        AdminInstanceUpdateVersion::MissingCurrentVersion => {
            panic!("explicit current_version should be accepted")
        }
    }
}

#[test]
fn non_server_binary_requires_explicit_current_version() {
    assert!(matches!(
        admin_instance_update_version("dcc-mcp-cli", None),
        AdminInstanceUpdateVersion::MissingCurrentVersion
    ));
}
