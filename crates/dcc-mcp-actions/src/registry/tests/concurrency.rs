//! Thread-safety / concurrency tests for ToolRegistry.

use super::fixtures::make_action;
use super::*;

// ── Concurrency ─────────────────────────────────────────────────────────────

#[test]
fn test_registry_thread_safety() {
    use std::sync::Arc;
    use std::thread;

    let reg = Arc::new(ToolRegistry::new());
    let mut handles = vec![];

    for i in 0..10 {
        let reg = Arc::clone(&reg);
        handles.push(thread::spawn(move || {
            reg.register_action(ToolMeta {
                name: format!("action_{i}"),
                description: format!("Action {i}"),
                dcc: "test".into(),
                ..Default::default()
            });
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(reg.len(), 10);
}

#[test]
fn test_registry_concurrent_reads_while_writing() {
    use std::sync::Arc;
    use std::thread;

    let reg = Arc::new(ToolRegistry::new());
    // Pre-populate
    for i in 0..5 {
        reg.register_action(make_action(&format!("pre_{i}"), "maya"));
    }

    let mut handles = vec![];
    // Readers
    for _ in 0..4 {
        let r = Arc::clone(&reg);
        handles.push(thread::spawn(move || {
            for _ in 0..20 {
                let _ = r.list_actions(None);
                let _ = r.get_all_dccs();
            }
        }));
    }
    // Writer
    {
        let r = Arc::clone(&reg);
        handles.push(thread::spawn(move || {
            for i in 0..5 {
                r.register_action(make_action(&format!("new_{i}"), "blender"));
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
    // At least 5 pre-populated + up to 5 new
    assert!(reg.len() >= 5);
}
