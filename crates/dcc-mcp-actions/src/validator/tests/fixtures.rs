//! Shared test helpers for validator tests.

use super::*;
use crate::registry::ToolMeta;

pub fn make_meta_with_schema(schema: Value) -> ToolMeta {
    ToolMeta {
        name: "test_action".into(),
        dcc: "maya".into(),
        input_schema: schema,
        ..Default::default()
    }
}
