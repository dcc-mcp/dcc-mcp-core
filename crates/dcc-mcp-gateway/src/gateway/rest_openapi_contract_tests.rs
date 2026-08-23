use super::build_gateway_openapi_document;

#[test]
fn gateway_openapi_keeps_shared_envelope_schemas() {
    let doc = build_gateway_openapi_document("1.2.3");
    let schemas = doc["components"]["schemas"]
        .as_object()
        .expect("schemas object");
    for schema in [
        "ServiceError",
        "GatewayFeedbackReport",
        "GatewayFeedbackReceipt",
        "SearchRequest",
        "SearchResponse",
        "LoadSkillRequest",
        "GatewayLoadSkillResponse",
        "GatewaySkillLoadResponse",
        "GatewayGroupActionResponse",
        "UnloadSkillRequest",
        "SkillLifecycleResponse",
        "DescribeRequest",
        "DescribeResponse",
        "CallRequest",
        "CallOutcome",
        "GatewayDirectCallRequest",
        "GatewayPolicyDenial",
        "GatewayBatchCallItem",
        "GatewayCallBatchRequest",
    ] {
        assert!(
            schemas.contains_key(schema),
            "gateway OpenAPI schema set missing {schema}"
        );
    }
}

#[test]
fn gateway_openapi_requires_verified_update_downloads() {
    let doc = build_gateway_openapi_document("1.2.3");
    let check = &doc["components"]["schemas"]["GatewayUpdateCheckResponse"];
    let download = &doc["components"]["schemas"]["GatewayUpdateDownloadResponse"];

    assert_eq!(
        check["properties"]["sha256"]["pattern"],
        "^[0-9a-fA-F]{64}$"
    );
    assert_eq!(
        download["properties"]["sha256"]["pattern"],
        "^[0-9a-fA-F]{64}$"
    );
    assert_eq!(
        doc["paths"]["/v1/update/download/{binary_name}"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/GatewayUpdateDownloadResponse"
    );
}
