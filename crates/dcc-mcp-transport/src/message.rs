//! Wire protocol message types.
//!
//! Uses MessagePack for serialization (safe + fast).
//! Frame format: `[4-byte big-endian length][MessagePack payload]`
//!
//! ## Message envelope
//!
//! All messages on the wire are wrapped in a [`MessageEnvelope`] that carries a
//! discriminator tag so the receiver can dispatch without knowing the type upfront:
//!
//! | Variant      | Direction          | Purpose                              |
//! |--------------|--------------------|--------------------------------------|
//! | `Request`    | client → DCC       | Method invocation                    |
//! | `Response`   | DCC → client       | Method result                        |
//! | `Ping`       | either direction   | Heartbeat probe                      |
//! | `Pong`       | either direction   | Heartbeat reply (echoes ping id)     |
//! | `Notify`     | either direction   | One-way event / notification         |
//! | `Shutdown`   | either direction   | Graceful connection close request    |

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::error::{TransportError, TransportResult};

// ── Leaf message types ────────────────────────────────────────────────────

/// Request message sent from client to DCC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    /// Unique request ID for correlation.
    pub id: Uuid,
    /// Method name (e.g. "execute_python", "list_tools").
    pub method: String,
    /// Serialized parameters (MessagePack bytes).
    #[serde(with = "serde_bytes")]
    pub params: Vec<u8>,
}

/// Response message sent from DCC to client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Response {
    /// Correlating request ID.
    pub id: Uuid,
    /// Whether the request succeeded.
    pub success: bool,
    /// Serialized result (MessagePack bytes).
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,
    /// Error message (if `success == false`).
    pub error: Option<String>,
}

/// Heartbeat ping message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ping {
    /// Unique ping ID — the corresponding [`Pong`] must echo this value.
    pub id: Uuid,
    /// Timestamp (milliseconds since UNIX epoch) when the ping was created.
    pub timestamp_ms: u64,
}

impl Ping {
    /// Create a new ping with a random ID and the current timestamp.
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }
}

impl Default for Ping {
    fn default() -> Self {
        Self::new()
    }
}

/// Heartbeat pong (reply to a [`Ping`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pong {
    /// Must match the [`Ping::id`] that triggered this reply.
    pub id: Uuid,
    /// Timestamp (milliseconds since UNIX epoch) when the pong was created.
    pub timestamp_ms: u64,
}

impl Pong {
    /// Create a pong that echoes a given [`Ping`].
    pub fn from_ping(ping: &Ping) -> Self {
        Self {
            id: ping.id,
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }

    /// Calculate the round-trip time based on the original ping timestamp.
    ///
    /// Returns `None` if the pong timestamp is earlier than the ping
    /// (clock skew) or if the ping timestamp is not known.
    pub fn rtt_ms(&self, ping: &Ping) -> Option<u64> {
        self.timestamp_ms.checked_sub(ping.timestamp_ms)
    }
}

/// One-way notification / event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Notification {
    /// Optional correlation ID (e.g. to associate with a prior request).
    pub id: Option<Uuid>,
    /// Event topic or category (e.g. "scene_changed", "render_complete").
    pub topic: String,
    /// Serialized event data (MessagePack bytes).
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

/// Graceful shutdown request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShutdownMessage {
    /// Optional human-readable reason.
    pub reason: Option<String>,
}

// ── MessageEnvelope ───────────────────────────────────────────────────────

/// Tagged message envelope for the wire protocol.
///
/// Every frame on the wire contains one `MessageEnvelope`. The `serde` tag
/// allows the receiver to dispatch to the correct handler without knowing the
/// message type upfront.
///
/// ```ignore
/// use dcc_mcp_transport::message::{MessageEnvelope, Request, Ping};
///
/// // Sending
/// let envelope = MessageEnvelope::from(request);
/// framed.send(&envelope).await?;
///
/// // Receiving
/// let envelope: MessageEnvelope = framed.recv().await?;
/// match envelope {
///     MessageEnvelope::Request(req) => handle_request(req),
///     MessageEnvelope::Ping(ping) => send_pong(ping),
///     _ => {}
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageEnvelope {
    /// Method invocation (client → DCC).
    Request(Request),
    /// Method result (DCC → client).
    Response(Response),
    /// Heartbeat probe (either direction).
    Ping(Ping),
    /// Heartbeat reply (either direction).
    Pong(Pong),
    /// One-way event / notification (either direction).
    Notify(Notification),
    /// Graceful connection close request (either direction).
    Shutdown(ShutdownMessage),
}

impl MessageEnvelope {
    /// Check if this is a control message (Ping, Pong, or Shutdown).
    pub fn is_control(&self) -> bool {
        matches!(self, Self::Ping(_) | Self::Pong(_) | Self::Shutdown(_))
    }

    /// Check if this is a data message (Request, Response, or Notify).
    pub fn is_data(&self) -> bool {
        !self.is_control()
    }

    /// Get the message type name as a static string.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Request(_) => "request",
            Self::Response(_) => "response",
            Self::Ping(_) => "ping",
            Self::Pong(_) => "pong",
            Self::Notify(_) => "notify",
            Self::Shutdown(_) => "shutdown",
        }
    }
}

impl From<Request> for MessageEnvelope {
    fn from(req: Request) -> Self {
        Self::Request(req)
    }
}

impl From<Response> for MessageEnvelope {
    fn from(resp: Response) -> Self {
        Self::Response(resp)
    }
}

impl From<Ping> for MessageEnvelope {
    fn from(ping: Ping) -> Self {
        Self::Ping(ping)
    }
}

impl From<Pong> for MessageEnvelope {
    fn from(pong: Pong) -> Self {
        Self::Pong(pong)
    }
}

impl From<Notification> for MessageEnvelope {
    fn from(notif: Notification) -> Self {
        Self::Notify(notif)
    }
}

impl From<ShutdownMessage> for MessageEnvelope {
    fn from(msg: ShutdownMessage) -> Self {
        Self::Shutdown(msg)
    }
}

impl std::fmt::Display for MessageEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(r) => write!(f, "Request(id={}, method={})", r.id, r.method),
            Self::Response(r) => write!(f, "Response(id={}, success={})", r.id, r.success),
            Self::Ping(p) => write!(f, "Ping(id={})", p.id),
            Self::Pong(p) => write!(f, "Pong(id={})", p.id),
            Self::Notify(n) => write!(f, "Notify(topic={})", n.topic),
            Self::Shutdown(s) => write!(
                f,
                "Shutdown(reason={})",
                s.reason.as_deref().unwrap_or("none")
            ),
        }
    }
}

// ── Encoding / decoding helpers ───────────────────────────────────────────

/// Encode a message to wire format: `[4-byte length][MessagePack payload]`.
pub fn encode_message<T: Serialize>(msg: &T) -> TransportResult<Vec<u8>> {
    let payload =
        rmp_serde::to_vec(msg).map_err(|e| TransportError::Serialization(e.to_string()))?;
    let len = payload.len() as u32;
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&payload);
    Ok(buf)
}

/// Decode a MessagePack payload (without the length prefix).
pub fn decode_message<T: for<'de> Deserialize<'de>>(data: &[u8]) -> TransportResult<T> {
    rmp_serde::from_slice(data).map_err(|e| TransportError::Serialization(e.to_string()))
}

/// Helper module for serde_bytes on Vec<u8>.
mod serde_bytes {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(bytes, s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        serde::Deserialize::deserialize(d)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Leaf type roundtrip tests ──

    mod leaf_types {
        use super::*;

        #[test]
        fn test_request_roundtrip() {
            let req = Request {
                id: Uuid::new_v4(),
                method: "execute_python".to_string(),
                params: b"hello".to_vec(),
            };

            let encoded = encode_message(&req).unwrap();
            let len = u32::from_be_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]) as usize;
            assert_eq!(len, encoded.len() - 4);

            let decoded: Request = decode_message(&encoded[4..]).unwrap();
            assert_eq!(decoded.id, req.id);
            assert_eq!(decoded.method, req.method);
            assert_eq!(decoded.params, req.params);
        }

        #[test]
        fn test_response_roundtrip() {
            let resp = Response {
                id: Uuid::new_v4(),
                success: true,
                payload: b"result data".to_vec(),
                error: None,
            };

            let encoded = encode_message(&resp).unwrap();
            let decoded: Response = decode_message(&encoded[4..]).unwrap();
            assert_eq!(decoded.id, resp.id);
            assert!(decoded.success);
            assert_eq!(decoded.payload, resp.payload);
            assert!(decoded.error.is_none());
        }

        #[test]
        fn test_error_response() {
            let resp = Response {
                id: Uuid::new_v4(),
                success: false,
                payload: vec![],
                error: Some("something went wrong".to_string()),
            };

            let encoded = encode_message(&resp).unwrap();
            let decoded: Response = decode_message(&encoded[4..]).unwrap();
            assert!(!decoded.success);
            assert_eq!(decoded.error.as_deref(), Some("something went wrong"));
        }

        #[test]
        fn test_ping_new() {
            let ping = Ping::new();
            assert_ne!(ping.id, Uuid::nil());
            assert!(ping.timestamp_ms > 0);
        }

        #[test]
        fn test_ping_default() {
            let ping = Ping::default();
            assert_ne!(ping.id, Uuid::nil());
        }

        #[test]
        fn test_pong_from_ping() {
            let ping = Ping::new();
            let pong = Pong::from_ping(&ping);
            assert_eq!(pong.id, ping.id);
            assert!(pong.timestamp_ms >= ping.timestamp_ms);
        }

        #[test]
        fn test_pong_rtt() {
            let ping = Ping {
                id: Uuid::new_v4(),
                timestamp_ms: 1000,
            };
            let pong = Pong {
                id: ping.id,
                timestamp_ms: 1050,
            };
            assert_eq!(pong.rtt_ms(&ping), Some(50));
        }

        #[test]
        fn test_pong_rtt_clock_skew() {
            let ping = Ping {
                id: Uuid::new_v4(),
                timestamp_ms: 2000,
            };
            let pong = Pong {
                id: ping.id,
                timestamp_ms: 1000,
            };
            assert_eq!(pong.rtt_ms(&ping), None);
        }

        #[test]
        fn test_notification_roundtrip() {
            let notif = Notification {
                id: Some(Uuid::new_v4()),
                topic: "scene_changed".to_string(),
                data: b"some event data".to_vec(),
            };

            let encoded = encode_message(&notif).unwrap();
            let decoded: Notification = decode_message(&encoded[4..]).unwrap();
            assert_eq!(decoded.id, notif.id);
            assert_eq!(decoded.topic, notif.topic);
            assert_eq!(decoded.data, notif.data);
        }

        #[test]
        fn test_shutdown_message() {
            let msg = ShutdownMessage {
                reason: Some("server restarting".to_string()),
            };
            let encoded = encode_message(&msg).unwrap();
            let decoded: ShutdownMessage = decode_message(&encoded[4..]).unwrap();
            assert_eq!(decoded.reason.as_deref(), Some("server restarting"));
        }

        #[test]
        fn test_shutdown_message_no_reason() {
            let msg = ShutdownMessage { reason: None };
            let encoded = encode_message(&msg).unwrap();
            let decoded: ShutdownMessage = decode_message(&encoded[4..]).unwrap();
            assert!(decoded.reason.is_none());
        }
    }

    // ── MessageEnvelope tests ──

    mod envelope {
        use super::*;

        #[test]
        fn test_envelope_request_roundtrip() {
            let req = Request {
                id: Uuid::new_v4(),
                method: "test".to_string(),
                params: vec![1, 2, 3],
            };
            let envelope = MessageEnvelope::from(req.clone());
            let encoded = encode_message(&envelope).unwrap();
            let decoded: MessageEnvelope = decode_message(&encoded[4..]).unwrap();
            assert_eq!(decoded, MessageEnvelope::Request(req));
        }

        #[test]
        fn test_envelope_response_roundtrip() {
            let resp = Response {
                id: Uuid::new_v4(),
                success: true,
                payload: vec![4, 5, 6],
                error: None,
            };
            let envelope = MessageEnvelope::from(resp.clone());
            let encoded = encode_message(&envelope).unwrap();
            let decoded: MessageEnvelope = decode_message(&encoded[4..]).unwrap();
            assert_eq!(decoded, MessageEnvelope::Response(resp));
        }

        #[test]
        fn test_envelope_ping_roundtrip() {
            let ping = Ping::new();
            let envelope = MessageEnvelope::from(ping.clone());
            let encoded = encode_message(&envelope).unwrap();
            let decoded: MessageEnvelope = decode_message(&encoded[4..]).unwrap();
            assert_eq!(decoded, MessageEnvelope::Ping(ping));
        }

        #[test]
        fn test_envelope_pong_roundtrip() {
            let ping = Ping::new();
            let pong = Pong::from_ping(&ping);
            let envelope = MessageEnvelope::from(pong.clone());
            let encoded = encode_message(&envelope).unwrap();
            let decoded: MessageEnvelope = decode_message(&encoded[4..]).unwrap();
            assert_eq!(decoded, MessageEnvelope::Pong(pong));
        }

        #[test]
        fn test_envelope_notify_roundtrip() {
            let notif = Notification {
                id: None,
                topic: "render_complete".to_string(),
                data: b"frame 42".to_vec(),
            };
            let envelope = MessageEnvelope::from(notif.clone());
            let encoded = encode_message(&envelope).unwrap();
            let decoded: MessageEnvelope = decode_message(&encoded[4..]).unwrap();
            assert_eq!(decoded, MessageEnvelope::Notify(notif));
        }

        #[test]
        fn test_envelope_shutdown_roundtrip() {
            let msg = ShutdownMessage {
                reason: Some("maintenance".to_string()),
            };
            let envelope = MessageEnvelope::from(msg.clone());
            let encoded = encode_message(&envelope).unwrap();
            let decoded: MessageEnvelope = decode_message(&encoded[4..]).unwrap();
            assert_eq!(decoded, MessageEnvelope::Shutdown(msg));
        }

        #[test]
        fn test_envelope_is_control() {
            assert!(MessageEnvelope::Ping(Ping::new()).is_control());
            assert!(MessageEnvelope::Pong(Pong::from_ping(&Ping::new())).is_control());
            assert!(MessageEnvelope::Shutdown(ShutdownMessage { reason: None }).is_control());

            let req = Request {
                id: Uuid::new_v4(),
                method: "test".to_string(),
                params: vec![],
            };
            assert!(!MessageEnvelope::Request(req).is_control());
        }

        #[test]
        fn test_envelope_is_data() {
            let req = Request {
                id: Uuid::new_v4(),
                method: "test".to_string(),
                params: vec![],
            };
            assert!(MessageEnvelope::Request(req).is_data());

            let resp = Response {
                id: Uuid::new_v4(),
                success: true,
                payload: vec![],
                error: None,
            };
            assert!(MessageEnvelope::Response(resp).is_data());

            let notif = Notification {
                id: None,
                topic: "evt".to_string(),
                data: vec![],
            };
            assert!(MessageEnvelope::Notify(notif).is_data());

            assert!(!MessageEnvelope::Ping(Ping::new()).is_data());
        }

        #[test]
        fn test_envelope_type_name() {
            assert_eq!(
                MessageEnvelope::Request(Request {
                    id: Uuid::new_v4(),
                    method: "t".to_string(),
                    params: vec![]
                })
                .type_name(),
                "request"
            );
            assert_eq!(
                MessageEnvelope::Response(Response {
                    id: Uuid::new_v4(),
                    success: true,
                    payload: vec![],
                    error: None
                })
                .type_name(),
                "response"
            );
            assert_eq!(MessageEnvelope::Ping(Ping::new()).type_name(), "ping");
            assert_eq!(
                MessageEnvelope::Pong(Pong::from_ping(&Ping::new())).type_name(),
                "pong"
            );
            assert_eq!(
                MessageEnvelope::Notify(Notification {
                    id: None,
                    topic: "x".to_string(),
                    data: vec![]
                })
                .type_name(),
                "notify"
            );
            assert_eq!(
                MessageEnvelope::Shutdown(ShutdownMessage { reason: None }).type_name(),
                "shutdown"
            );
        }

        #[test]
        fn test_envelope_display() {
            let req = MessageEnvelope::Request(Request {
                id: Uuid::nil(),
                method: "execute_python".to_string(),
                params: vec![],
            });
            let display = format!("{req}");
            assert!(display.contains("Request"));
            assert!(display.contains("execute_python"));

            let ping = MessageEnvelope::Ping(Ping::new());
            let display = format!("{ping}");
            assert!(display.contains("Ping"));

            let shutdown = MessageEnvelope::Shutdown(ShutdownMessage { reason: None });
            let display = format!("{shutdown}");
            assert!(display.contains("Shutdown"));
            assert!(display.contains("none"));
        }
    }
}
