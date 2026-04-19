//! DCC-Link wire frame and ipckit adapters.
//!
//! This module introduces an explicit DCC-Link frame format:
//! `[u32 len][u8 type][u64 seq][msgpack body]`.
//! It provides:
//! - `DccLinkFrame` encode/decode helpers
//! - `IpcChannelAdapter` over `ipckit::IpcChannel<Vec<u8>>`
//! - `GracefulIpcChannelAdapter` over `ipckit::GracefulIpcChannel<Vec<u8>>`
//! - `SocketServerAdapter` over `ipckit::SocketServer`

use std::time::Duration;

use ipckit::{GracefulIpcChannel, IpcChannel, SocketServer, SocketServerConfig};

use crate::error::{TransportError, TransportResult};

/// DCC-Link message type tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DccLinkType {
    Call = 1,
    Reply = 2,
    Err = 3,
    Progress = 4,
    Cancel = 5,
    Push = 6,
    Ping = 7,
    Pong = 8,
}

impl TryFrom<u8> for DccLinkType {
    type Error = TransportError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Call),
            2 => Ok(Self::Reply),
            3 => Ok(Self::Err),
            4 => Ok(Self::Progress),
            5 => Ok(Self::Cancel),
            6 => Ok(Self::Push),
            7 => Ok(Self::Ping),
            8 => Ok(Self::Pong),
            other => Err(TransportError::Serialization(format!(
                "unknown DccLinkType: {other}"
            ))),
        }
    }
}

/// A DCC-Link frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DccLinkFrame {
    pub msg_type: DccLinkType,
    pub seq: u64,
    pub body: Vec<u8>,
}

impl DccLinkFrame {
    const HEADER_LEN: usize = 1 + 8;

    /// Encode to `[len][type][seq][body]` where `len = 1 + 8 + body.len()`.
    pub fn encode(&self) -> TransportResult<Vec<u8>> {
        let payload_len = Self::HEADER_LEN + self.body.len();
        let len_u32 = u32::try_from(payload_len).map_err(|_| TransportError::FrameTooLarge {
            size: payload_len,
            max_size: u32::MAX as usize,
        })?;

        let mut out = Vec::with_capacity(4 + payload_len);
        out.extend_from_slice(&len_u32.to_be_bytes());
        out.push(self.msg_type as u8);
        out.extend_from_slice(&self.seq.to_be_bytes());
        out.extend_from_slice(&self.body);
        Ok(out)
    }

    /// Decode from a full frame buffer including the 4-byte length prefix.
    pub fn decode(frame: &[u8]) -> TransportResult<Self> {
        if frame.len() < 4 + Self::HEADER_LEN {
            return Err(TransportError::Serialization(
                "dcc-link frame too short".to_string(),
            ));
        }

        let declared_len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        let actual_len = frame.len().saturating_sub(4);
        if declared_len != actual_len {
            return Err(TransportError::Serialization(format!(
                "dcc-link frame length mismatch: declared={declared_len}, actual={actual_len}"
            )));
        }

        let msg_type = DccLinkType::try_from(frame[4])?;
        let seq = u64::from_be_bytes([
            frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11], frame[12],
        ]);
        let body = frame[13..].to_vec();

        Ok(Self {
            msg_type,
            seq,
            body,
        })
    }
}

/// Thin adapter over `ipckit::IpcChannel<Vec<u8>>` using DCC-Link framing.
pub struct IpcChannelAdapter {
    inner: IpcChannel<Vec<u8>>,
}

impl IpcChannelAdapter {
    pub fn create(name: &str) -> TransportResult<Self> {
        let inner = IpcChannel::<Vec<u8>>::create(name).map_err(map_ipckit_err)?;
        Ok(Self { inner })
    }

    pub fn connect(name: &str) -> TransportResult<Self> {
        let inner = IpcChannel::<Vec<u8>>::connect(name).map_err(map_ipckit_err)?;
        Ok(Self { inner })
    }

    pub fn wait_for_client(&mut self) -> TransportResult<()> {
        self.inner.wait_for_client().map_err(map_ipckit_err)
    }

    pub fn send_frame(&mut self, frame: &DccLinkFrame) -> TransportResult<()> {
        let bytes = frame.encode()?;
        self.inner.send_bytes(&bytes).map_err(map_ipckit_err)
    }

    pub fn recv_frame(&mut self) -> TransportResult<DccLinkFrame> {
        let bytes = self.inner.recv_bytes().map_err(map_ipckit_err)?;
        DccLinkFrame::decode(&bytes)
    }
}

/// Thin adapter over `ipckit::GracefulIpcChannel<Vec<u8>>` using DCC-Link framing.
pub struct GracefulIpcChannelAdapter {
    inner: GracefulIpcChannel<Vec<u8>>,
}

impl GracefulIpcChannelAdapter {
    pub fn create(name: &str) -> TransportResult<Self> {
        let inner = GracefulIpcChannel::<Vec<u8>>::create(name).map_err(map_ipckit_err)?;
        Ok(Self { inner })
    }

    pub fn connect(name: &str) -> TransportResult<Self> {
        let inner = GracefulIpcChannel::<Vec<u8>>::connect(name).map_err(map_ipckit_err)?;
        Ok(Self { inner })
    }

    pub fn wait_for_client(&mut self) -> TransportResult<()> {
        self.inner.wait_for_client().map_err(map_ipckit_err)
    }

    pub fn send_frame(&mut self, frame: &DccLinkFrame) -> TransportResult<()> {
        let bytes = frame.encode()?;
        self.inner.send_bytes(&bytes).map_err(map_ipckit_err)
    }

    pub fn recv_frame(&mut self) -> TransportResult<DccLinkFrame> {
        let bytes = self.inner.recv_bytes().map_err(map_ipckit_err)?;
        DccLinkFrame::decode(&bytes)
    }

    pub fn shutdown(&self) {
        use ipckit::GracefulChannel;
        self.inner.shutdown();
    }
}

/// Minimal wrapper for `ipckit::SocketServer`.
pub struct SocketServerAdapter {
    inner: SocketServer,
}

impl SocketServerAdapter {
    pub fn new(
        path: &str,
        max_connections: usize,
        connection_timeout: Duration,
    ) -> TransportResult<Self> {
        let config = SocketServerConfig {
            path: path.to_string(),
            max_connections,
            connection_timeout,
            cleanup_on_start: true,
            buffer_size: 8192,
        };
        let inner = SocketServer::new(config).map_err(map_ipckit_err)?;
        Ok(Self { inner })
    }

    pub fn socket_path(&self) -> &str {
        self.inner.socket_path()
    }

    pub fn connection_count(&self) -> usize {
        self.inner.connection_count()
    }
}

fn map_ipckit_err(err: ipckit::IpcError) -> TransportError {
    TransportError::IpcConnectionFailed {
        address: "ipckit://local".to_string(),
        reason: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dcc_link_frame_roundtrip() {
        let frame = DccLinkFrame {
            msg_type: DccLinkType::Call,
            seq: 42,
            body: vec![1, 2, 3, 4],
        };
        let encoded = frame.encode().unwrap();
        let decoded = DccLinkFrame::decode(&encoded).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn dcc_link_frame_rejects_length_mismatch() {
        let mut bad = vec![0, 0, 0, 16, DccLinkType::Call as u8];
        bad.extend_from_slice(&42_u64.to_be_bytes());
        bad.extend_from_slice(&[1, 2, 3]);
        let err = DccLinkFrame::decode(&bad).unwrap_err();
        assert!(err.to_string().contains("length mismatch"));
    }

    #[test]
    fn dcc_link_type_rejects_unknown_tag() {
        let err = DccLinkType::try_from(255).unwrap_err();
        assert!(err.to_string().contains("unknown DccLinkType"));
    }
}
