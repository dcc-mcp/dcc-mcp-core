use dcc_mcp_models::DccMcpError as DomainError;
use dcc_mcp_protocols::ToolCallErrorEnvelope;

#[test]
fn domain_error_and_tool_call_envelope_have_distinct_roles() {
    let domain = DomainError::Validation("radius must be positive".to_string());
    assert_eq!(domain.code(), "validation");

    let envelope = ToolCallErrorEnvelope::new(
        "instance",
        "INVALID_RADIUS",
        "The Maya sphere radius must be positive.",
    )
    .with_hint("Pass radius greater than zero");
    assert_eq!(envelope.layer, "instance");
    assert_eq!(envelope.code, "INVALID_RADIUS");
    assert!(envelope.trace_id.is_some());
}

#[test]
#[allow(deprecated)]
fn legacy_protocol_name_is_a_wire_compatible_alias() {
    let envelope = dcc_mcp_protocols::DccMcpError::new(
        "dcc",
        "DCC_CMD_FAILED",
        "Photoshop rejected the command.",
    );
    let canonical: ToolCallErrorEnvelope = envelope;

    assert_eq!(canonical.code, "DCC_CMD_FAILED");
}
