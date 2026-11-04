//! One policy for automatic CLI update side effects, independent of gateway lifecycle.

#[derive(Clone, Copy)]
pub(crate) struct UpdateMaintenance {
    pub(crate) enabled: bool,
}

impl UpdateMaintenance {
    pub(crate) fn from_skip_flag(skip: bool) -> Self {
        Self { enabled: !skip }
    }

    pub(crate) fn run<T>(self, action: impl FnOnce() -> T) -> Option<T> {
        self.enabled.then(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn opt_out_invokes_none_of_the_three_automatic_update_paths() {
        let policy = UpdateMaintenance::from_skip_flag(true);
        for path in ["staged apply", "marketplace check", "cache refresh"] {
            let calls = Cell::new(0);
            assert!(
                policy.run(|| calls.set(calls.get() + 1)).is_none(),
                "{path}"
            );
            assert_eq!(calls.get(), 0, "{path}");
        }
    }

    #[test]
    fn default_policy_preserves_automatic_update_execution() {
        let policy = UpdateMaintenance::from_skip_flag(false);
        let calls = Cell::new(0);
        for _ in 0..3 {
            assert_eq!(
                policy.run(|| {
                    calls.set(calls.get() + 1);
                    true
                }),
                Some(true)
            );
        }
        assert_eq!(calls.get(), 3);
    }
}
