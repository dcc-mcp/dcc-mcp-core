use dcc_mcp_gateway_core::capability_naming;
use uuid::Uuid;

#[allow(deprecated)]
use dcc_mcp_gateway_core::naming;

#[test]
#[allow(deprecated)]
fn legacy_naming_module_projects_the_canonical_api() {
    let instance_id =
        Uuid::parse_str("9c8e57c1-3849-4a7f-a586-e550537ca085").expect("fixture UUID should parse");

    assert_eq!(
        naming::instance_short(&instance_id),
        capability_naming::instance_short(&instance_id)
    );
    assert_eq!(
        naming::skill_tool_name("maya-rigging", "create_joint"),
        capability_naming::skill_tool_name("maya-rigging", "create_joint")
    );
}
