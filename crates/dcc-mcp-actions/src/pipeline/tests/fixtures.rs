//! Shared test helpers for pipeline tests.

use super::*;

pub fn make_pipeline_with_echo() -> ActionPipeline {
    let registry = ActionRegistry::new();
    registry.register_action(ActionMeta {
        name: "echo".into(),
        dcc: "mock".into(),
        ..Default::default()
    });
    let dispatcher = ActionDispatcher::new(registry);
    dispatcher.register_handler("echo", Ok);
    ActionPipeline::new(dispatcher)
}

pub fn make_pipeline_with_failing() -> ActionPipeline {
    let registry = ActionRegistry::new();
    let dispatcher = ActionDispatcher::new(registry);
    dispatcher.register_handler("fail", |_| Err("intentional failure".to_string()));
    ActionPipeline::new(dispatcher)
}
