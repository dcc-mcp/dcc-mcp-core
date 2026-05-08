//! Shared test helpers for pipeline tests.

use super::*;

pub fn make_pipeline_with_echo() -> ToolPipeline {
    let registry = ToolRegistry::new();
    registry.register_action(ToolMeta {
        name: "echo".into(),
        dcc: "mock".into(),
        ..Default::default()
    });
    let dispatcher = ToolDispatcher::new(registry);
    dispatcher.register_handler("echo", Ok);
    ToolPipeline::new(dispatcher)
}

pub fn make_pipeline_with_failing() -> ToolPipeline {
    let registry = ToolRegistry::new();
    let dispatcher = ToolDispatcher::new(registry);
    dispatcher.register_handler("fail", |_| Err("intentional failure".to_string()));
    ToolPipeline::new(dispatcher)
}
