//! The single error type for the async client (`DESIGN.md §7`).

use std::time::Duration;

/// Which wire protocol a connected device speaks. HD2020 probing lands in the
/// HD2020 dispatch subsystem; v0 handshake always yields [`ProtocolKind::Sdk2`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolKind {
    /// SDK 2.0 XML command protocol.
    Sdk2,
    /// HD2020 Gen6 realtime protocol.
    Hd2020,
}

/// Everything the `huidu` client can fail with. No `Box<dyn Error>` in the
/// public surface — every case a caller might match on is a variant.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An underlying transport I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A framing, codec, XML, or device-level protocol error from `huidu-proto`.
    #[error("protocol error: {0}")]
    Proto(#[from] huidu_proto::ProtoError),

    /// A reply frame carried a command code the current step did not expect.
    #[error("unexpected reply: expected command 0x{expected:04x}, got 0x{got:04x}")]
    UnexpectedReply {
        /// The raw command word the step awaited.
        expected: u16,
        /// The raw command word that actually arrived.
        got: u16,
    },

    /// The connection closed before a reply arrived.
    #[error("connection closed before reply")]
    ConnectionClosed,

    /// A previous command's future was cancelled mid-round-trip, desyncing the
    /// stream. Every later command fails this way until the caller reconnects
    /// (`DESIGN.md §4.4`).
    #[error("connection poisoned by cancelled command")]
    Poisoned,

    /// A handshake phase failed. `phase` is 1 (version), 2 (`GetIFVersion`), or
    /// 3 (`GetDeviceInfo`).
    #[error("handshake failed at phase {phase}: {source}")]
    Handshake {
        /// The 1-based handshake phase that failed.
        phase: u8,
        /// The underlying cause.
        source: Box<Error>,
    },

    /// A command was invoked that the connected protocol does not support.
    #[error("operation not supported by {0:?} protocol")]
    UnsupportedForProtocol(ProtocolKind),

    /// A round-trip did not complete within `DeviceConfig::timeout`.
    #[error("timeout after {0:?}")]
    Timeout(Duration),
}

/// Result specialized to the client's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
