use std::sync::atomic::Ordering;

use crate::adapters::{DccConnection, DccError, DccErrorCode, DccResult};

use super::MockDccAdapter;

impl DccConnection for MockDccAdapter {
    fn connect(&mut self) -> DccResult<()> {
        self.connect_count.fetch_add(1, Ordering::Relaxed);

        if self.connect_should_fail {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: self.connect_error_message.clone(),
                details: None,
                recoverable: true,
            });
        }

        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn disconnect(&mut self) -> DccResult<()> {
        self.disconnect_count.fetch_add(1, Ordering::Relaxed);
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn health_check(&self) -> DccResult<u64> {
        self.health_check_count.fetch_add(1, Ordering::Relaxed);

        if !self.connected.load(Ordering::SeqCst) {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: "Not connected".to_string(),
                details: None,
                recoverable: true,
            });
        }

        Ok(self.health_check_latency_ms)
    }
}
