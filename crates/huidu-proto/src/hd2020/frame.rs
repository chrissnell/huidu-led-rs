//! HD2020 Gen6 realtime framing.
//!
//! HD2020 is a *separate* wire protocol that shares only the TCP port with SDK
//! 2.0 (`DESIGN.md §6`): it does not use the `<len:u16le><cmd:u16le>` envelope
//! from `crate::codec`. This module owns its own frame shape and command set so
//! the two protocols stay cleanly isolated under `huidu-proto::hd2020`.
//!
//! # Frame shape (design-derived — reconcile before locking)
//!
//! ```text
//! ┌──────┬──────┬───────────┬───────────────┬──────────┐
//! │ 0xA5 │ cmd  │ len:u16le │ payload[len]  │ checksum │
//! │  u8  │  u8  │           │               │   u8     │
//! └──────┴──────┴───────────┴───────────────┴──────────┘
//! ```
//!
//! `checksum` is the low byte of the sum of every byte from `cmd` through the
//! end of `payload` (inclusive). The `0xA5` start byte lets a reader resync.
//!
//! **Provenance.** The exact Gen6 start byte, command numbering, length span,
//! and checksum are not specified in `DESIGN.md` and no capture or Go reference
//! (`hd2020_gen6_test.go`) is available in this workspace. The values here are a
//! self-consistent, round-trippable placeholder chosen so the encoder, decoder,
//! bitmap layer, and golden fixtures can be built and tested now. When the Go
//! reference or a real capture lands, only the constants in this file and the
//! fixture bytes change; the module's shape and public API do not. This mirrors
//! how `huidu-proto` core locked its `<len>` span (see the core plan).

use crate::error::ProtoError;
use bytes::{BufMut, Bytes, BytesMut};

/// Frame start / resync byte.
pub const START: u8 = 0xA5;

/// Bytes of fixed framing overhead: start(1) + cmd(1) + len(2) + checksum(1).
const OVERHEAD: usize = 5;

/// HD2020 Gen6 realtime command codes (design-derived; see the module docs).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hd2020Cmd {
    /// Replace the realtime display with a rendered text bitmap.
    RealtimeText = 0x01,
    /// Blank the realtime display.
    Clear = 0x02,
    /// Set panel brightness; payload is a single 0–255 level byte.
    Brightness = 0x03,
}

impl Hd2020Cmd {
    /// Parse a raw command byte, rejecting unknown codes.
    pub fn from_u8(v: u8) -> Result<Self, ProtoError> {
        Ok(match v {
            0x01 => Self::RealtimeText,
            0x02 => Self::Clear,
            0x03 => Self::Brightness,
            other => {
                return Err(ProtoError::Hd2020(format!(
                    "unknown HD2020 command 0x{other:02x}"
                )))
            }
        })
    }

    /// The raw command byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// A decoded HD2020 Gen6 frame that owns its payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hd2020Frame {
    pub cmd: Hd2020Cmd,
    pub payload: Bytes,
}

impl Hd2020Frame {
    /// Build a frame from a command and any byte container.
    pub fn new(cmd: Hd2020Cmd, payload: impl Into<Bytes>) -> Self {
        Self {
            cmd,
            payload: payload.into(),
        }
    }

    /// Low byte of the sum of the `cmd` byte, both little-endian length bytes,
    /// and every payload byte.
    fn checksum(cmd: u8, len: u16, payload: &[u8]) -> u8 {
        let [lo, hi] = len.to_le_bytes();
        let mut sum = cmd.wrapping_add(lo).wrapping_add(hi);
        for &b in payload {
            sum = sum.wrapping_add(b);
        }
        sum
    }

    /// Serialize the frame to its on-wire bytes.
    pub fn encode(&self) -> Result<Bytes, ProtoError> {
        let len = u16::try_from(self.payload.len()).map_err(|_| {
            ProtoError::Hd2020(format!(
                "HD2020 payload {} bytes exceeds u16 length field",
                self.payload.len()
            ))
        })?;
        let cmd = self.cmd.as_u8();
        let mut buf = BytesMut::with_capacity(OVERHEAD + self.payload.len());
        buf.put_u8(START);
        buf.put_u8(cmd);
        buf.put_u16_le(len);
        buf.put_slice(&self.payload);
        buf.put_u8(Self::checksum(cmd, len, &self.payload));
        Ok(buf.freeze())
    }

    /// Decode one frame from the front of `buf`, returning it and the number of
    /// bytes consumed. `Ok(None)` means `buf` does not yet hold a whole frame.
    pub fn decode(buf: &[u8]) -> Result<Option<(Self, usize)>, ProtoError> {
        if buf.len() < OVERHEAD {
            return Ok(None);
        }
        if buf[0] != START {
            return Err(ProtoError::Hd2020(format!(
                "bad HD2020 start byte 0x{:02x}, expected 0x{START:02x}",
                buf[0]
            )));
        }
        let cmd_byte = buf[1];
        let len = u16::from_le_bytes([buf[2], buf[3]]) as usize;
        let total = OVERHEAD + len;
        if buf.len() < total {
            return Ok(None);
        }
        let payload = &buf[4..4 + len];
        let want = Self::checksum(cmd_byte, len as u16, payload);
        let got = buf[4 + len];
        if want != got {
            return Err(ProtoError::Hd2020(format!(
                "HD2020 checksum mismatch: computed 0x{want:02x}, frame carried 0x{got:02x}"
            )));
        }
        let cmd = Hd2020Cmd::from_u8(cmd_byte)?;
        let frame = Self {
            cmd,
            payload: Bytes::copy_from_slice(payload),
        };
        Ok(Some((frame, total)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_roundtrips_through_u8() {
        for c in [
            Hd2020Cmd::RealtimeText,
            Hd2020Cmd::Clear,
            Hd2020Cmd::Brightness,
        ] {
            assert_eq!(Hd2020Cmd::from_u8(c.as_u8()).unwrap(), c);
        }
    }

    #[test]
    fn unknown_cmd_errors() {
        let err = Hd2020Cmd::from_u8(0x7f).unwrap_err();
        assert!(matches!(err, ProtoError::Hd2020(_)));
    }

    #[test]
    fn encode_decode_roundtrips() {
        let frame = Hd2020Frame::new(Hd2020Cmd::RealtimeText, Bytes::from_static(&[1, 2, 3, 4]));
        let bytes = frame.encode().unwrap();
        let (decoded, used) = Hd2020Frame::decode(&bytes).unwrap().unwrap();
        assert_eq!(decoded, frame);
        assert_eq!(used, bytes.len());
    }

    #[test]
    fn brightness_frame_is_well_formed() {
        let frame = Hd2020Frame::new(Hd2020Cmd::Brightness, Bytes::from_static(&[0x80]));
        let b = frame.encode().unwrap();
        // start, cmd, len(1,0), payload 0x80, checksum = (0x03 + 0x01 + 0x80) & 0xff.
        assert_eq!(&b[..5], &[0xA5, 0x03, 0x01, 0x00, 0x80]);
        assert_eq!(b[5], 0x03u8.wrapping_add(0x01).wrapping_add(0x80));
    }

    #[test]
    fn decode_waits_for_full_frame() {
        let frame = Hd2020Frame::new(Hd2020Cmd::Clear, Bytes::new());
        let bytes = frame.encode().unwrap();
        assert!(Hd2020Frame::decode(&bytes[..bytes.len() - 1])
            .unwrap()
            .is_none());
        assert!(Hd2020Frame::decode(&bytes).unwrap().is_some());
    }

    #[test]
    fn decode_rejects_bad_start_byte() {
        let mut bytes = Hd2020Frame::new(Hd2020Cmd::Clear, Bytes::new())
            .encode()
            .unwrap()
            .to_vec();
        bytes[0] = 0x00;
        let err = Hd2020Frame::decode(&bytes).unwrap_err();
        assert!(matches!(err, ProtoError::Hd2020(_)));
    }

    #[test]
    fn decode_rejects_corrupt_checksum() {
        let mut bytes = Hd2020Frame::new(Hd2020Cmd::Brightness, Bytes::from_static(&[0x40]))
            .encode()
            .unwrap()
            .to_vec();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        let err = Hd2020Frame::decode(&bytes).unwrap_err();
        assert!(matches!(err, ProtoError::Hd2020(_)));
    }

    #[test]
    fn decode_reports_bytes_consumed_leaving_trailing_data() {
        let one = Hd2020Frame::new(Hd2020Cmd::Clear, Bytes::new())
            .encode()
            .unwrap();
        let mut two = one.to_vec();
        two.extend_from_slice(&[0xde, 0xad]); // trailing bytes of a next frame
        let (_, used) = Hd2020Frame::decode(&two).unwrap().unwrap();
        assert_eq!(used, one.len());
    }
}
