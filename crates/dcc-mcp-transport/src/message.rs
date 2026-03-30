//! Wire protocol message types.
//!
//! Uses MessagePack for serialization (safe + fast).
//! Frame format: `[4-byte big-endian length][MessagePack payload]`

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{TransportError, TransportResult};

/// Request message sent from client to DCC.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_roundtrip() {
        let req = Request {
            id: Uuid::new_v4(),
            method: "execute_python".to_string(),
            params: b"hello".to_vec(),
        };

        let encoded = encode_message(&req).unwrap();
        // First 4 bytes are the length prefix
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
}
