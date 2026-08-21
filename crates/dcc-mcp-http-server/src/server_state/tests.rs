use super::*;

fn builder() -> ServerStateBuilder {
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        Arc::clone(&registry),
        Arc::clone(&dispatcher),
    ));
    ServerState::builder(registry, dispatcher, catalog)
}

#[test]
fn feature_flags_are_copied_as_one_server_state_snapshot() {
    let features = FeatureFlags {
        lazy_actions: true,
        bare_tool_names: false,
        enable_resources: false,
        enable_prompts: false,
        exclude_skill_stubs_from_tools_list: true,
        exclude_group_stubs_from_tools_list: true,
        standalone_main_thread_execution: true,
        ..FeatureFlags::default()
    };

    let state = builder().with_features(features).build();

    assert!(state.features.lazy_actions);
    assert!(!state.features.bare_tool_names);
    assert!(!state.features.enable_resources);
    assert!(!state.features.enable_prompts);
    assert!(state.features.exclude_skill_stubs_from_tools_list);
    assert!(state.features.exclude_group_stubs_from_tools_list);
    assert!(state.features.standalone_main_thread_execution);
}

#[test]
fn compatibility_builders_update_the_same_feature_snapshot() {
    let state = builder()
        .with_lazy_actions(true)
        .with_bare_tool_names(false)
        .with_resources_enabled(false)
        .with_prompts_enabled(false)
        .with_exclude_skill_stubs_from_tools_list(true)
        .with_exclude_group_stubs_from_tools_list(true)
        .with_standalone_main_thread_execution(true)
        .build();

    assert!(state.features.lazy_actions);
    assert!(!state.features.bare_tool_names);
    assert!(!state.features.enable_resources);
    assert!(!state.features.enable_prompts);
    assert!(state.features.exclude_skill_stubs_from_tools_list);
    assert!(state.features.exclude_group_stubs_from_tools_list);
    assert!(state.features.standalone_main_thread_execution);
}
