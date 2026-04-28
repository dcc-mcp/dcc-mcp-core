//! Shared test helpers for validator tests.

use super::*;
use crate::registry::ActionMeta;

pub fn make_meta_with_schema(schema: Value) -> ActionMeta {
    ActionMeta {
        name: "test_action".into(),
        dcc: "maya".into(),
        input_schema: schema,
        ..Default::default()
    }
}
