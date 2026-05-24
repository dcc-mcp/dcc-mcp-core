use std::collections::HashMap;

use serde::Deserialize;

use super::TrafficCaptureError;

#[derive(Debug, Deserialize)]
pub(super) struct TrafficCaptureDocument {
    pub(super) enabled: Option<bool>,
    pub(super) sinks: Option<Vec<TrafficSinkDocument>>,
    pub(super) filters: Option<TrafficFilterDocument>,
    pub(super) redact: Option<Vec<HashMap<String, String>>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TrafficSinkDocument {
    pub(super) kind: String,
    pub(super) path: Option<String>,
    #[allow(dead_code)]
    pub(super) ring_buffer: Option<usize>,
}

impl TrafficSinkDocument {
    pub(super) fn path_required(&self) -> Result<String, TrafficCaptureError> {
        self.path
            .as_ref()
            .filter(|path| !path.trim().is_empty())
            .cloned()
            .ok_or_else(|| TrafficCaptureError::SinkPathRequired {
                kind: self.kind.clone(),
            })
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct TrafficFilterDocument {
    pub(super) include: Option<Vec<HashMap<String, String>>>,
    pub(super) exclude: Option<Vec<HashMap<String, String>>>,
}
