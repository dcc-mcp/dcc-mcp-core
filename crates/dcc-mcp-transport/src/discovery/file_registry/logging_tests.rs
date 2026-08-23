use super::*;
use std::{collections::HashMap, fmt, sync::Arc};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

#[derive(Clone, Default)]
struct EventRecorder {
    events: Arc<Mutex<Vec<HashMap<String, String>>>>,
}

#[derive(Default)]
struct FieldRecorder(HashMap<String, String>);

impl Visit for FieldRecorder {
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }
}

impl Subscriber for EventRecorder {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = FieldRecorder::default();
        event.record(&mut fields);
        self.events.lock().unwrap().push(fields.0);
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[test]
fn duplicate_service_keys_log_the_loaded_membership_count() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();
    let first = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let mut duplicate = first.clone();
    duplicate.port = 18813;
    duplicate.touch();
    std::fs::write(
        registry.registry_file_path(),
        serde_json::to_string(&[first, duplicate]).unwrap(),
    )
    .unwrap();

    let recorder = EventRecorder::default();
    let dispatch = tracing::Dispatch::new(recorder.clone());
    tracing::dispatcher::with_default(&dispatch, || registry.load_from_file().unwrap());

    assert_eq!(registry.len(), 1, "duplicate keys collapse to one member");
    let events = recorder.events.lock().unwrap();
    let fields = events
        .iter()
        .find(|fields| {
            fields
                .get("message")
                .is_some_and(|message| message.contains("registry membership changed"))
        })
        .expect("membership change event");
    assert_eq!(fields.get("count").map(String::as_str), Some("1"));
    assert_eq!(fields.get("added").map(String::as_str), Some("1"));
    assert_eq!(fields.get("removed").map(String::as_str), Some("0"));
}
