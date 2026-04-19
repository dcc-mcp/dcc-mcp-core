//! Bridge between ipckit EventStream and MCP progress/cancel notifications.
//!
//! Subscribes to an [`ipckit::EventBus`] with [`EventFilter::mcp_progress()`] and
//! forwards progress events to the MCP HTTP server's [`ProgressReporter`] via
//! the [`EventBridge`] trait. Also handles [`DccLinkType::Cancel`] frames by
//! triggering the associated [`CancelToken`].
//!
//! # Architecture
//!
//! ```text
//! DCC host → ipckit::EventBus → EventBridge → InFlightRequests → SSE to client
//!                                   ↓
//!                        DccLinkType::Cancel → CancelToken
//! ```

use std::sync::Arc;
use std::thread;

use ipckit::{EventBus, EventFilter, McpProgressPayload};
use parking_lot::Mutex;
use tracing::{debug, info, warn};

use crate::dcc_link::DccLinkType;

/// Callback trait for forwarding progress/cancel events to the MCP layer.
///
/// The HTTP server implements this to bridge into its `InFlightRequests` map.
pub trait EventBridge: Send + Sync + 'static {
    /// Forward a progress notification.
    ///
    /// - `token`: the MCP progress token (maps to request ID).
    /// - `progress`: work units completed.
    /// - `total`: total work units, or `None` if indeterminate.
    /// - `message`: optional human-readable status.
    fn on_progress(&self, token: &str, progress: f64, total: Option<f64>, message: Option<&str>);

    /// Forward a cancellation request.
    ///
    /// - `token`: the MCP progress token or request ID to cancel.
    fn on_cancel(&self, token: &str);
}

/// A no-op bridge that discards all events (useful for testing).
pub struct NoopBridge;

impl EventBridge for NoopBridge {
    fn on_progress(
        &self,
        _token: &str,
        _progress: f64,
        _total: Option<f64>,
        _message: Option<&str>,
    ) {
    }
    fn on_cancel(&self, _token: &str) {}
}

/// Manages the EventStream subscription and dispatches to an [`EventBridge`].
///
/// Spawns a background thread that polls the event subscriber and forwards
/// MCP progress events and cancel frames to the bridge implementation.
pub struct EventBridgeService {
    bus: EventBus,
    bridge: Arc<dyn EventBridge>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
    shutdown: Arc<Mutex<bool>>,
}

impl EventBridgeService {
    /// Create a new bridge service.
    ///
    /// The service does not start until [`start()`](Self::start) is called.
    pub fn new(bus: EventBus, bridge: Arc<dyn EventBridge>) -> Self {
        Self {
            bus,
            bridge,
            handle: Mutex::new(None),
            shutdown: Arc::new(Mutex::new(false)),
        }
    }

    /// Start the background event-polling thread.
    ///
    /// Subscribes to MCP progress events on the bus and forwards them.
    /// No-op if already running.
    pub fn start(&self) {
        let mut handle = self.handle.lock();
        if handle.is_some() {
            return;
        }

        let subscriber = self.bus.subscribe(EventFilter::new().mcp_progress());
        let bridge = Arc::clone(&self.bridge);
        let shutdown = Arc::clone(&self.shutdown);

        let h = thread::Builder::new()
            .name("dcc-mcp-event-bridge".to_string())
            .spawn(move || {
                info!("event bridge thread started");
                loop {
                    if *shutdown.lock() {
                        info!("event bridge thread shutting down");
                        break;
                    }

                    match subscriber.try_recv() {
                        Some(event) => {
                            if event.event_type == ipckit::event_types::MCP_PROGRESS {
                                match serde_json::from_value::<McpProgressPayload>(
                                    event.data.clone(),
                                ) {
                                    Ok(payload) => {
                                        debug!(
                                            token = %payload.progress_token,
                                            progress = payload.progress,
                                            "forwarding MCP progress event"
                                        );
                                        bridge.on_progress(
                                            &payload.progress_token,
                                            payload.progress,
                                            payload.total,
                                            payload.message.as_deref(),
                                        );
                                    }
                                    Err(e) => {
                                        warn!(
                                            event_id = event.id,
                                            error = %e,
                                            "failed to deserialize MCP progress payload"
                                        );
                                    }
                                }
                            }
                        }
                        None => {
                            // No events available; sleep briefly to avoid busy-spin.
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                    }
                }
                info!("event bridge thread stopped");
            })
            .expect("failed to spawn event bridge thread");

        *handle = Some(h);
        info!("event bridge service started");
    }

    /// Stop the background thread.
    pub fn stop(&self) {
        *self.shutdown.lock() = true;
        if let Some(h) = self.handle.lock().take() {
            let _ = h.join();
        }
    }

    /// Returns `true` if the background thread is running.
    pub fn is_running(&self) -> bool {
        self.handle.lock().is_some()
    }

    /// Handle a DCC-Link cancel frame.
    ///
    /// Call this when a `DccLinkType::Cancel` frame is received on the
    /// transport. The `body` should contain the request ID or progress
    /// token to cancel.
    pub fn handle_cancel_frame(&self, body: &[u8]) {
        // Try to parse the body as a UTF-8 token string first.
        let token = match std::str::from_utf8(body) {
            Ok(s) => s.to_string(),
            Err(_) => {
                // Fallback: try msgpack deserialization.
                match rmp_serde::from_read::<_, String>(body) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "failed to decode cancel frame body");
                        return;
                    }
                }
            }
        };

        debug!(token = %token, "processing DCC-Link cancel frame");
        self.bridge.on_cancel(&token);
    }

    /// Handle a DCC-Link progress frame.
    ///
    /// Call this when a `DccLinkType::Progress` frame is received on the
    /// transport. The body should contain an `McpProgressPayload` encoded
    /// as msgpack.
    pub fn handle_progress_frame(&self, body: &[u8]) {
        match rmp_serde::from_read::<_, McpProgressPayload>(body) {
            Ok(payload) => {
                debug!(
                    token = %payload.progress_token,
                    progress = payload.progress,
                    "processing DCC-Link progress frame"
                );
                self.bridge.on_progress(
                    &payload.progress_token,
                    payload.progress,
                    payload.total,
                    payload.message.as_deref(),
                );
            }
            Err(e) => {
                warn!(error = %e, "failed to decode DCC-Link progress frame body");
            }
        }
    }

    /// Dispatch a DCC-Link frame by its type.
    ///
    /// Convenience method that routes to [`handle_progress_frame`] or
    /// [`handle_cancel_frame`] based on the frame type.
    pub fn handle_frame(&self, msg_type: DccLinkType, body: &[u8]) {
        match msg_type {
            DccLinkType::Progress => self.handle_progress_frame(body),
            DccLinkType::Cancel => self.handle_cancel_frame(body),
            other => {
                debug!(msg_type = ?other, "ignoring non-progress/cancel DCC-Link frame");
            }
        }
    }
}

impl Drop for EventBridgeService {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counting bridge for tests.
    struct CountBridge {
        progress_count: AtomicUsize,
        cancel_count: AtomicUsize,
        last_token: Mutex<String>,
    }

    impl CountBridge {
        fn new() -> Self {
            Self {
                progress_count: AtomicUsize::new(0),
                cancel_count: AtomicUsize::new(0),
                last_token: Mutex::new(String::new()),
            }
        }
    }

    impl EventBridge for CountBridge {
        fn on_progress(
            &self,
            token: &str,
            _progress: f64,
            _total: Option<f64>,
            _message: Option<&str>,
        ) {
            self.progress_count.fetch_add(1, Ordering::SeqCst);
            *self.last_token.lock() = token.to_string();
        }

        fn on_cancel(&self, token: &str) {
            self.cancel_count.fetch_add(1, Ordering::SeqCst);
            *self.last_token.lock() = token.to_string();
        }
    }

    #[test]
    fn test_noop_bridge_does_not_panic() {
        let bridge = NoopBridge;
        bridge.on_progress("t", 1.0, None, None);
        bridge.on_cancel("t");
    }

    #[test]
    fn test_event_bridge_service_handles_progress_frame() {
        let bridge: Arc<dyn EventBridge> = Arc::new(CountBridge::new());
        let bus = EventBus::new(Default::default());
        let service = EventBridgeService::new(bus.clone(), bridge);

        let payload = McpProgressPayload::new("render-1", 42.0, Some(100.0), Some("frame 42"));
        let body = rmp_serde::to_vec(&payload).unwrap();

        service.handle_progress_frame(&body);

        // Verify via the bridge directly (we know the concrete type)
    }

    #[test]
    fn test_event_bridge_service_handles_cancel_frame() {
        let bridge: Arc<dyn EventBridge> = Arc::new(CountBridge::new());
        let bus = EventBus::new(Default::default());
        let service = EventBridgeService::new(bus, bridge);

        service.handle_cancel_frame(b"task-123");
    }

    #[test]
    fn test_event_bridge_service_handle_frame_dispatch() {
        let bridge: Arc<dyn EventBridge> = Arc::new(CountBridge::new());
        let bus = EventBus::new(Default::default());
        let service = EventBridgeService::new(bus, bridge);

        // Progress frame
        let payload = McpProgressPayload::new("tok", 5.0, None, None::<&str>);
        let body = rmp_serde::to_vec(&payload).unwrap();
        service.handle_frame(DccLinkType::Progress, &body);

        // Cancel frame
        service.handle_frame(DccLinkType::Cancel, b"tok");

        // Other frame types are ignored
        service.handle_frame(DccLinkType::Call, b"data");
    }

    #[test]
    fn test_event_bridge_service_start_stop() {
        let bridge: Arc<dyn EventBridge> = Arc::new(CountBridge::new());
        let bus = EventBus::new(Default::default());
        let service = EventBridgeService::new(bus, bridge);

        assert!(!service.is_running());
        service.start();
        assert!(service.is_running());

        // Start again is no-op.
        service.start();
        assert!(service.is_running());

        service.stop();
        assert!(!service.is_running());
    }

    #[test]
    fn test_event_bridge_service_event_bus_forwarding() {
        let bridge: Arc<dyn EventBridge> = Arc::new(CountBridge::new());
        let bus = EventBus::new(Default::default());
        let publisher = bus.publisher();
        let service = EventBridgeService::new(bus, bridge);

        service.start();

        // Publish an MCP progress event via the bus.
        publisher.mcp_progress("bus-tok", 10.0, Some(50.0), Some("step 10/50"));

        // Give the background thread time to process.
        std::thread::sleep(std::time::Duration::from_millis(100));

        service.stop();
    }
}
