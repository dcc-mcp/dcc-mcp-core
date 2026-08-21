use dcc_mcp_http_types::config::GatewaySettings;

fn accepts_settings(_: GatewaySettings) {}

#[test]
#[allow(deprecated)]
fn historical_gateway_config_aliases_gateway_settings() {
    let legacy = dcc_mcp_http_types::config::GatewayConfig::default();
    accepts_settings(legacy);
}

#[test]
fn settings_preserve_the_serialized_http_contract() {
    let settings = GatewaySettings {
        gateway_port: 9765,
        remote_host: Some("127.0.0.1".into()),
        ..GatewaySettings::default()
    };

    let value = serde_json::to_value(&settings).unwrap();
    let decoded: GatewaySettings = serde_json::from_value(value.clone()).unwrap();

    assert_eq!(value["gateway_port"], 9765);
    assert_eq!(value["remote_host"], "127.0.0.1");
    assert_eq!(decoded.gateway_port, settings.gateway_port);
    assert_eq!(decoded.remote_host, settings.remote_host);
}
