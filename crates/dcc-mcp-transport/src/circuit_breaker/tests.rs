use std::time::Duration;

use super::*;

fn make_cb(threshold: u32) -> CircuitBreaker {
    CircuitBreaker::new(
        "test",
        CircuitBreakerConfig {
            failure_threshold: threshold,
            recovery_timeout: Duration::from_secs(60),
            probe_success_threshold: 1,
            failure_window: None,
        },
    )
}

fn make_fast_recovery_cb(threshold: u32) -> CircuitBreaker {
    CircuitBreaker::new(
        "test-fast",
        CircuitBreakerConfig {
            failure_threshold: threshold,
            recovery_timeout: Duration::from_millis(1),
            probe_success_threshold: 1,
            failure_window: None,
        },
    )
}

// ── CircuitState ─────────────────────────────────────────────────────────────

mod state {
    use super::*;

    #[test]
    fn test_state_display() {
        assert_eq!(CircuitState::Closed.to_string(), "closed");
        assert_eq!(CircuitState::Open.to_string(), "open");
        assert_eq!(CircuitState::HalfOpen.to_string(), "half_open");
    }

    #[test]
    fn test_initial_state_is_closed() {
        let cb = make_cb(3);
        assert_eq!(cb.state(), CircuitState::Closed);
    }
}

// ── Closed state ──────────────────────────────────────────────────────────────

mod closed_state {
    use super::*;

    #[test]
    fn test_allows_requests_when_closed() {
        let cb = make_cb(3);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_success_keeps_circuit_closed() {
        let cb = make_cb(3);
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_failures_below_threshold_stay_closed() {
        let cb = make_cb(3);
        let _ = cb.allow_request();
        cb.record_failure();
        let _ = cb.allow_request();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.consecutive_failures(), 2);
    }

    #[test]
    fn test_success_resets_failure_counter() {
        let cb = make_cb(3);
        let _ = cb.allow_request();
        cb.record_failure();
        let _ = cb.allow_request();
        cb.record_failure();
        let _ = cb.allow_request();
        cb.record_success(); // resets counter
        assert_eq!(cb.consecutive_failures(), 0);
    }
}

// ── Open state ───────────────────────────────────────────────────────────────

mod open_state {
    use super::*;

    #[test]
    fn test_circuit_opens_after_threshold() {
        let cb = make_cb(3);
        for _ in 0..3 {
            let _ = cb.allow_request();
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_open_circuit_rejects_requests() {
        let cb = make_cb(1);
        let _ = cb.allow_request();
        cb.record_failure(); // trips the circuit
        assert!(!cb.allow_request()); // fast-fail
    }

    #[test]
    fn test_open_circuit_increments_rejected_counter() {
        let cb = make_cb(1);
        let _ = cb.allow_request();
        cb.record_failure();

        let _ = cb.allow_request(); // rejected
        let _ = cb.allow_request(); // rejected
        assert_eq!(cb.stats().total_rejected, 2);
    }

    #[test]
    fn test_trips_counter_increments() {
        let cb = make_cb(1);
        let _ = cb.allow_request();
        cb.record_failure(); // trip 1
        cb.reset();
        let _ = cb.allow_request();
        cb.record_failure(); // trip 2
        assert_eq!(cb.stats().trips, 2);
    }
}

// ── HalfOpen state ───────────────────────────────────────────────────────────

mod half_open_state {
    use super::*;

    #[test]
    fn test_transitions_to_half_open_after_timeout() {
        let cb = make_fast_recovery_cb(1);
        let _ = cb.allow_request();
        cb.record_failure(); // opens

        assert_eq!(cb.state(), CircuitState::Open);

        std::thread::sleep(Duration::from_millis(2)); // wait for recovery

        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_half_open_probe_success_closes_circuit() {
        let cb = make_fast_recovery_cb(1);
        let _ = cb.allow_request();
        cb.record_failure(); // opens

        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        let _ = cb.allow_request(); // probe
        cb.record_success(); // closes

        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_probe_failure_reopens_circuit() {
        let cb = make_fast_recovery_cb(1);
        let _ = cb.allow_request();
        cb.record_failure(); // opens

        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        let _ = cb.allow_request(); // probe
        cb.record_failure(); // re-opens

        assert_eq!(cb.state(), CircuitState::Open);
        assert_eq!(cb.stats().trips, 2); // opened twice
    }

    #[test]
    fn test_multiple_probe_successes_required() {
        let cb = CircuitBreaker::new(
            "test",
            CircuitBreakerConfig {
                failure_threshold: 1,
                recovery_timeout: Duration::from_millis(1),
                probe_success_threshold: 2, // need 2 successes to close
                failure_window: None,
            },
        );
        let _ = cb.allow_request();
        cb.record_failure(); // opens

        std::thread::sleep(Duration::from_millis(2));
        // State: HalfOpen

        let _ = cb.allow_request();
        cb.record_success(); // 1/2 successes
        assert_eq!(cb.state(), CircuitState::HalfOpen); // still half-open

        // Second probe
        let _ = cb.allow_request();
        cb.record_success(); // 2/2 successes → close
        assert_eq!(cb.state(), CircuitState::Closed);
    }
}

// ── Reset ────────────────────────────────────────────────────────────────────

mod reset {
    use super::*;

    #[test]
    fn test_reset_from_open() {
        let cb = make_cb(1);
        let _ = cb.allow_request();
        cb.record_failure(); // opens

        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.consecutive_failures(), 0);
    }

    #[test]
    fn test_reset_allows_requests_again() {
        let cb = make_cb(1);
        let _ = cb.allow_request();
        cb.record_failure(); // opens
        assert!(!cb.allow_request()); // blocked

        cb.reset();
        assert!(cb.allow_request()); // allowed again
    }
}

// ── Statistics ───────────────────────────────────────────────────────────────

mod stats {
    use super::*;

    #[test]
    fn test_stats_success_rate() {
        let cb = make_cb(10);
        // 3 successes, 1 failure (but record_failure also increments total)
        let _ = cb.allow_request();
        cb.record_success();
        let _ = cb.allow_request();
        cb.record_success();
        let _ = cb.allow_request();
        cb.record_success();
        // record_failure increments total_requests too
        cb.record_failure();

        let stats = cb.stats();
        assert_eq!(stats.total_successes, 3);
        // total_failures = 1, total_requests includes both allow_request + record_failure calls
        assert!(stats.failure_rate() > 0.0);
    }

    #[test]
    fn test_stats_empty_success_rate() {
        let cb = make_cb(5);
        assert_eq!(cb.stats().success_rate(), 1.0); // no requests → 100%
        assert_eq!(cb.stats().failure_rate(), 0.0);
    }

    #[test]
    fn test_stats_rejected_counted() {
        let cb = make_cb(1);
        let _ = cb.allow_request();
        cb.record_failure(); // trips

        for _ in 0..5 {
            let _ = cb.allow_request(); // all rejected
        }
        assert_eq!(cb.stats().total_rejected, 5);
    }
}

// ── call() helper ────────────────────────────────────────────────────────────

mod call_helper {
    use super::*;

    #[test]
    fn test_call_success() {
        let cb = make_cb(3);
        let result: TransportResult<i32> = cb.call(|| -> Result<i32, String> { Ok(42) });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(cb.stats().total_successes, 1);
    }

    #[test]
    fn test_call_failure_records_failure() {
        let cb = make_cb(3);
        let _: TransportResult<()> = cb.call(|| -> Result<(), String> { Err("oops".to_string()) });
        assert_eq!(cb.consecutive_failures(), 1);
    }

    #[test]
    fn test_call_returns_circuit_open_when_tripped() {
        let cb = make_cb(1);
        // Trip the circuit
        let _: TransportResult<()> = cb.call(|| -> Result<(), String> { Err("fail".to_string()) });

        // Next call should get CircuitOpen error
        let result: TransportResult<()> = cb.call(|| -> Result<(), String> { Ok(()) });
        assert!(matches!(result, Err(TransportError::CircuitOpen { .. })));
    }

    #[test]
    fn test_call_trips_after_threshold() {
        let cb = make_cb(2);
        for _ in 0..2 {
            let _: TransportResult<()> = cb.call(|| -> Result<(), String> { Err("e".to_string()) });
        }
        assert_eq!(cb.state(), CircuitState::Open);
    }
}

// ── Debug ─────────────────────────────────────────────────────────────────────

mod debug {
    use super::*;

    #[test]
    fn test_circuit_breaker_debug() {
        let cb = make_cb(3);
        let s = format!("{cb:?}");
        assert!(s.contains("CircuitBreaker"));
    }

    #[test]
    fn test_circuit_breaker_name() {
        let cb = CircuitBreaker::with_defaults("maya-18812");
        assert_eq!(cb.name(), "maya-18812");
    }

    #[test]
    fn test_circuit_breaker_clone() {
        let cb1 = make_cb(3);
        let cb2 = cb1.clone();
        let _ = cb1.allow_request();
        cb1.record_failure();
        // Clone shares state
        assert_eq!(cb2.consecutive_failures(), 1);
    }
}

// ── CircuitBreakerRegistry ───────────────────────────────────────────────────

mod registry {
    use super::*;

    #[test]
    fn test_registry_get_or_create() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
        let cb1 = registry.get_or_create("maya");
        let cb2 = registry.get_or_create("maya");

        // Same key should share state
        let _ = cb1.allow_request();
        cb1.record_failure();
        assert_eq!(cb2.consecutive_failures(), 1);
    }

    #[test]
    fn test_registry_different_endpoints_isolated() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
        let maya_cb = registry.get_or_create("maya");
        let blender_cb = registry.get_or_create("blender");

        // Trip maya
        for _ in 0..5 {
            let _ = maya_cb.allow_request();
            maya_cb.record_failure();
        }

        // Blender should be unaffected
        assert_eq!(blender_cb.state(), CircuitState::Closed);
        assert_eq!(maya_cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_registry_len() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
        assert_eq!(registry.len(), 0);
        registry.get_or_create("a");
        registry.get_or_create("b");
        registry.get_or_create("c");
        assert_eq!(registry.len(), 3);
    }

    #[test]
    fn test_registry_remove() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
        registry.get_or_create("maya");
        assert_eq!(registry.len(), 1);
        assert!(registry.remove("maya"));
        assert_eq!(registry.len(), 0);
        assert!(!registry.remove("maya")); // second remove returns false
    }

    #[test]
    fn test_registry_snapshot_sorted() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
        registry.get_or_create("z_maya");
        registry.get_or_create("a_blender");
        registry.get_or_create("m_houdini");

        let snapshot = registry.snapshot();
        assert_eq!(snapshot[0].0, "a_blender");
        assert_eq!(snapshot[1].0, "m_houdini");
        assert_eq!(snapshot[2].0, "z_maya");
        for (_, state) in &snapshot {
            assert_eq!(*state, CircuitState::Closed);
        }
    }

    #[test]
    fn test_registry_is_empty() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
        assert!(registry.is_empty());
        registry.get_or_create("x");
        assert!(!registry.is_empty());
    }

    #[test]
    fn test_registry_register_custom_config() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
        let cb = registry.register(
            "critical-dcc",
            CircuitBreakerConfig {
                failure_threshold: 1, // strict: 1 failure opens immediately
                ..Default::default()
            },
        );
        let _ = cb.allow_request();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_registry_debug() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
        let s = format!("{registry:?}");
        assert!(s.contains("CircuitBreakerRegistry"));
    }
}
