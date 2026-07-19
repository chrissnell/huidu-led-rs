//! The single error type returned by everything in this crate.

/// Errors produced while framing, decoding, or reassembling wire messages.
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    /// An underlying transport I/O error. Required so this type can be a
    /// `tokio_util::codec` `Decoder`/`Encoder` error, which must be
    /// `From<std::io::Error>`.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A frame or payload was shorter than the minimum its shape requires.
    #[error("frame too short: expected {expected} bytes, got {got}")]
    ShortFrame { expected: usize, got: usize },

    /// The command field held a value we do not recognize.
    #[error("unknown command code: 0x{0:04x}")]
    UnknownCmd(u16),

    /// A frame's payload exceeds what the u16 length prefix can describe.
    #[error("frame payload too large: {0} bytes exceeds u16 length field")]
    FrameTooLarge(usize),

    /// An SDK fragment did not arrive at the next expected byte offset.
    #[error("sdk fragment offset {got} does not match expected {expected}")]
    FragmentGap { expected: u32, got: u32 },

    /// An SDK fragment would push the reassembled buffer past its declared length.
    #[error("sdk fragment overflows declared total length {total}: offset {offset} + {chunk} bytes")]
    FragmentOverflow { total: u32, offset: u32, chunk: usize },
}
