//! Command codes and the owned frame value the codec hands out.

use crate::error::ProtoError;
use bytes::Bytes;

/// Every SDK 2.0 / file-transfer command code. HD2020 codes are added by the
/// HD2020 subsystem plan.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdCode {
    VersionAsk = 0x2001,
    VersionReply = 0x2002,
    SdkCmd = 0x2003,
    SdkReply = 0x2004,
    Heartbeat = 0x2005,
    HeartbeatReply = 0x2006,
    FileStartAsk = 0x8001,
    FileStartReply = 0x8002,
    FileContentAsk = 0x8003,
    FileContentReply = 0x8004,
    FileEndAsk = 0x8005,
    FileEndReply = 0x8006,
}

impl CmdCode {
    /// Parse a raw command word, rejecting unknown codes.
    pub fn from_u16(v: u16) -> Result<Self, ProtoError> {
        Ok(match v {
            0x2001 => Self::VersionAsk,
            0x2002 => Self::VersionReply,
            0x2003 => Self::SdkCmd,
            0x2004 => Self::SdkReply,
            0x2005 => Self::Heartbeat,
            0x2006 => Self::HeartbeatReply,
            0x8001 => Self::FileStartAsk,
            0x8002 => Self::FileStartReply,
            0x8003 => Self::FileContentAsk,
            0x8004 => Self::FileContentReply,
            0x8005 => Self::FileEndAsk,
            0x8006 => Self::FileEndReply,
            other => return Err(ProtoError::UnknownCmd(other)),
        })
    }

    /// The raw command word for this code.
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

/// A decoded frame that owns its payload — no lifetime tied to the read buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedFrame {
    pub cmd: CmdCode,
    pub payload: Bytes,
}

impl OwnedFrame {
    /// Build a frame from a command code and any byte container.
    pub fn new(cmd: CmdCode, payload: impl Into<Bytes>) -> Self {
        Self {
            cmd,
            payload: payload.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_code_roundtrips_through_u16() {
        let code = CmdCode::from_u16(0x2005).unwrap();
        assert_eq!(code, CmdCode::Heartbeat);
        assert_eq!(code.as_u16(), 0x2005);
    }

    #[test]
    fn unknown_code_errors() {
        let err = CmdCode::from_u16(0x9999).unwrap_err();
        assert!(matches!(err, ProtoError::UnknownCmd(0x9999)));
    }
}
