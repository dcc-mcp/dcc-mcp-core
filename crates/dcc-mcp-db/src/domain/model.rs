//! Small value objects shared across persistence features.

/// Bounded retention window for time-series or append-only tables (days).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetentionDays(u32);

impl RetentionDays {
    /// Clamp to `[1, 3650]` so operators cannot accidentally pass `0` or absurd values.
    #[must_use]
    pub const fn new(days: u32) -> Self {
        let d = if days == 0 {
            1
        } else if days > 3650 {
            3650
        } else {
            days
        };
        Self(d)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_clamps() {
        assert_eq!(RetentionDays::new(0).get(), 1);
        assert_eq!(RetentionDays::new(10_000).get(), 3650);
        assert_eq!(RetentionDays::new(30).get(), 30);
    }
}
