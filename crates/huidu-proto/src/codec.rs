//! `tokio_util::codec` Decoder/Encoder for the raw `<len><cmd><payload>` frame.

use crate::error::ProtoError;
use crate::frame::{CmdCode, OwnedFrame};
use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

/// Bytes in the little-endian length prefix.
const LEN_BYTES: usize = 2;
/// Bytes in the little-endian command field.
const CMD_BYTES: usize = 2;

/// Stateless codec for the raw framing layer.
pub struct HuiduCodec;

impl Decoder for HuiduCodec {
    type Item = OwnedFrame;
    type Error = ProtoError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < LEN_BYTES {
            return Ok(None);
        }
        // `len` counts the cmd field plus the payload — see the plan's wire-format note.
        let len = u16::from_le_bytes([src[0], src[1]]) as usize;
        if len < CMD_BYTES {
            return Err(ProtoError::ShortFrame {
                expected: CMD_BYTES,
                got: len,
            });
        }
        let total = LEN_BYTES + len;
        if src.len() < total {
            src.reserve(total - src.len());
            return Ok(None);
        }
        // Peek the cmd word before consuming, so an UnknownCmd error leaves the
        // buffer untouched at the frame boundary rather than mid-frame.
        let cmd = CmdCode::from_u16(u16::from_le_bytes([src[LEN_BYTES], src[LEN_BYTES + 1]]))?;
        src.advance(LEN_BYTES + CMD_BYTES);
        let payload = src.split_to(len - CMD_BYTES).freeze();
        Ok(Some(OwnedFrame { cmd, payload }))
    }
}

impl Encoder<OwnedFrame> for HuiduCodec {
    type Error = ProtoError;

    fn encode(&mut self, frame: OwnedFrame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let len = CMD_BYTES + frame.payload.len();
        let len_u16 = u16::try_from(len).map_err(|_| ProtoError::FrameTooLarge(len))?;
        dst.reserve(LEN_BYTES + len);
        dst.put_u16_le(len_u16);
        dst.put_u16_le(frame.cmd.as_u16());
        dst.put_slice(&frame.payload);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn encodes_heartbeat() {
        let mut codec = HuiduCodec;
        let mut out = BytesMut::new();
        codec
            .encode(OwnedFrame::new(CmdCode::Heartbeat, Bytes::new()), &mut out)
            .unwrap();
        // len = 2 (cmd only), cmd = 0x2005, no payload.
        assert_eq!(&out[..], &[0x02, 0x00, 0x05, 0x20]);
    }

    #[test]
    fn encode_decode_roundtrips_with_payload() {
        let mut codec = HuiduCodec;
        let frame = OwnedFrame::new(CmdCode::SdkCmd, Bytes::from_static(b"abc"));
        let mut buf = BytesMut::new();
        codec.encode(frame.clone(), &mut buf).unwrap();
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded, frame);
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_waits_for_full_frame() {
        let mut codec = HuiduCodec;
        // Full frame is 0x02 0x00 0x05 0x20; feed only the length prefix first.
        let mut buf = BytesMut::from(&[0x02u8, 0x00][..]);
        assert!(codec.decode(&mut buf).unwrap().is_none());
        buf.extend_from_slice(&[0x05, 0x20]);
        let frame = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame.cmd, CmdCode::Heartbeat);
    }

    #[test]
    fn decode_rejects_unknown_command() {
        let mut codec = HuiduCodec;
        // len = 2, cmd = 0x9999 (unknown).
        let mut buf = BytesMut::from(&[0x02u8, 0x00, 0x99, 0x99][..]);
        let err = codec.decode(&mut buf).unwrap_err();
        assert!(matches!(err, ProtoError::UnknownCmd(0x9999)));
        // The buffer is left intact at the frame boundary, not advanced mid-frame.
        assert_eq!(&buf[..], &[0x02, 0x00, 0x99, 0x99]);
    }

    #[test]
    fn decode_handles_two_frames_in_one_buffer() {
        let mut codec = HuiduCodec;
        let mut buf = BytesMut::new();
        codec
            .encode(OwnedFrame::new(CmdCode::Heartbeat, Bytes::new()), &mut buf)
            .unwrap();
        codec
            .encode(OwnedFrame::new(CmdCode::HeartbeatReply, Bytes::new()), &mut buf)
            .unwrap();
        assert_eq!(codec.decode(&mut buf).unwrap().unwrap().cmd, CmdCode::Heartbeat);
        assert_eq!(
            codec.decode(&mut buf).unwrap().unwrap().cmd,
            CmdCode::HeartbeatReply
        );
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }
}
