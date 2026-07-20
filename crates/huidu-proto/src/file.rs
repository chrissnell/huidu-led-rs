//! File-transfer frame payloads (`0x8001`–`0x8006`, `DESIGN.md §8`).
//!
//! These are the binary bodies carried inside the file-upload frames — the
//! `huidu` crate drives the state machine over them. Like the SDK envelope this
//! is pure bytes with no I/O.
//!
//! **Provisional layout.** `DESIGN.md §8` specifies the state machine but not
//! the byte layout of these frames, the same gap Subsystem 1 flagged for the
//! raw `<len>` field. The layout below is clean and self-consistent but must be
//! confirmed against a real capture or the Go reference before any golden
//! fixtures are locked. All integers are little-endian; a string is a `u16le`
//! byte length followed by its UTF-8 bytes.

use crate::error::ProtoError;
use bytes::{BufMut, Bytes, BytesMut};

/// Raw MD5 digest length carried by the start/end frames.
const MD5_LEN: usize = 16;

/// A little-endian cursor over a frame payload that errors, rather than panics,
/// when a field runs past the end of the buffer.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Take `n` bytes, or fail if fewer remain.
    fn take(&mut self, n: usize) -> Result<&'a [u8], ProtoError> {
        let end = self.pos.checked_add(n).ok_or(ProtoError::ShortFrame {
            expected: n,
            got: self.buf.len().saturating_sub(self.pos),
        })?;
        if end > self.buf.len() {
            return Err(ProtoError::ShortFrame {
                expected: n,
                got: self.buf.len() - self.pos,
            });
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16, ProtoError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, ProtoError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, ProtoError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn md5(&mut self) -> Result<[u8; MD5_LEN], ProtoError> {
        let mut out = [0u8; MD5_LEN];
        out.copy_from_slice(self.take(MD5_LEN)?);
        Ok(out)
    }

    /// A `u16le` length followed by that many UTF-8 bytes.
    fn string(&mut self) -> Result<String, ProtoError> {
        let len = self.u16()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|e| ProtoError::Xml(format!("file frame string not utf-8: {e}")))
    }
}

/// Append a `u16le`-prefixed UTF-8 string, rejecting one too long for the prefix.
fn put_string(buf: &mut BytesMut, s: &str) -> Result<(), ProtoError> {
    let len = u16::try_from(s.len()).map_err(|_| ProtoError::FrameTooLarge(s.len()))?;
    buf.put_u16_le(len);
    buf.put_slice(s.as_bytes());
    Ok(())
}

/// `FileStartAsk` (`0x8001`): announce a transfer. The device replies with the
/// offset it already holds so an interrupted upload can resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStartAsk {
    pub name: String,
    pub file_type: String,
    pub size: u64,
    pub md5: [u8; MD5_LEN],
}

impl FileStartAsk {
    pub fn encode(&self) -> Result<Bytes, ProtoError> {
        let mut buf = BytesMut::new();
        put_string(&mut buf, &self.name)?;
        put_string(&mut buf, &self.file_type)?;
        buf.put_u64_le(self.size);
        buf.put_slice(&self.md5);
        Ok(buf.freeze())
    }

    pub fn parse(payload: &[u8]) -> Result<Self, ProtoError> {
        let mut r = Reader::new(payload);
        Ok(Self {
            name: r.string()?,
            file_type: r.string()?,
            size: r.u64()?,
            md5: r.md5()?,
        })
    }
}

/// `FileStartReply` (`0x8002`): the device's acceptance and resume point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStartReply {
    pub result: u16,
    pub resume_offset: u64,
}

impl FileStartReply {
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(10);
        buf.put_u16_le(self.result);
        buf.put_u64_le(self.resume_offset);
        buf.freeze()
    }

    pub fn parse(payload: &[u8]) -> Result<Self, ProtoError> {
        let mut r = Reader::new(payload);
        Ok(Self {
            result: r.u16()?,
            resume_offset: r.u64()?,
        })
    }
}

/// `FileContentAsk` (`0x8003`): one chunk of file bytes at `offset`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContentAsk {
    pub offset: u64,
    pub data: Bytes,
}

impl FileContentAsk {
    pub fn encode(&self) -> Result<Bytes, ProtoError> {
        let len = u32::try_from(self.data.len())
            .map_err(|_| ProtoError::FrameTooLarge(self.data.len()))?;
        let mut buf = BytesMut::with_capacity(12 + self.data.len());
        buf.put_u64_le(self.offset);
        buf.put_u32_le(len);
        buf.put_slice(&self.data);
        Ok(buf.freeze())
    }

    pub fn parse(payload: &[u8]) -> Result<Self, ProtoError> {
        let mut r = Reader::new(payload);
        let offset = r.u64()?;
        let len = r.u32()? as usize;
        let data = Bytes::copy_from_slice(r.take(len)?);
        Ok(Self { offset, data })
    }
}

/// `FileContentReply` (`0x8004`): acknowledgement plus the device's running byte
/// count. A non-zero `result` is retryable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileContentReply {
    pub result: u16,
    pub received: u64,
}

impl FileContentReply {
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(10);
        buf.put_u16_le(self.result);
        buf.put_u64_le(self.received);
        buf.freeze()
    }

    pub fn parse(payload: &[u8]) -> Result<Self, ProtoError> {
        let mut r = Reader::new(payload);
        Ok(Self {
            result: r.u16()?,
            received: r.u64()?,
        })
    }
}

/// `FileEndAsk` (`0x8005`): the whole-file MD5 for a final integrity check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileEndAsk {
    pub md5: [u8; MD5_LEN],
}

impl FileEndAsk {
    pub fn encode(&self) -> Bytes {
        Bytes::copy_from_slice(&self.md5)
    }

    pub fn parse(payload: &[u8]) -> Result<Self, ProtoError> {
        let mut r = Reader::new(payload);
        Ok(Self { md5: r.md5()? })
    }
}

/// `FileEndReply` (`0x8006`): the device's final accept/reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileEndReply {
    pub result: u16,
}

impl FileEndReply {
    pub fn encode(&self) -> Bytes {
        Bytes::copy_from_slice(&self.result.to_le_bytes())
    }

    pub fn parse(payload: &[u8]) -> Result<Self, ProtoError> {
        let mut r = Reader::new(payload);
        Ok(Self { result: r.u16()? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_ask_round_trips() {
        let ask = FileStartAsk {
            name: "clip.mp4".into(),
            file_type: "video".into(),
            size: 1_048_576,
            md5: [0xAB; MD5_LEN],
        };
        let bytes = ask.encode().unwrap();
        assert_eq!(FileStartAsk::parse(&bytes).unwrap(), ask);
    }

    #[test]
    fn start_reply_round_trips() {
        let reply = FileStartReply {
            result: 0,
            resume_offset: 4096,
        };
        assert_eq!(FileStartReply::parse(&reply.encode()).unwrap(), reply);
    }

    #[test]
    fn content_ask_round_trips() {
        let ask = FileContentAsk {
            offset: 8000,
            data: Bytes::from_static(b"the quick brown fox"),
        };
        let bytes = ask.encode().unwrap();
        assert_eq!(FileContentAsk::parse(&bytes).unwrap(), ask);
    }

    #[test]
    fn content_reply_round_trips() {
        let reply = FileContentReply {
            result: 0,
            received: 16000,
        };
        assert_eq!(FileContentReply::parse(&reply.encode()).unwrap(), reply);
    }

    #[test]
    fn end_frames_round_trip() {
        let ask = FileEndAsk {
            md5: [0x11; MD5_LEN],
        };
        assert_eq!(FileEndAsk::parse(&ask.encode()).unwrap(), ask);
        let reply = FileEndReply { result: 0 };
        assert_eq!(FileEndReply::parse(&reply.encode()).unwrap(), reply);
    }

    #[test]
    fn parse_rejects_truncated_payload() {
        // A start reply needs 10 bytes; give it 4.
        let err = FileStartReply::parse(&[0, 0, 0, 0]).unwrap_err();
        assert!(matches!(err, ProtoError::ShortFrame { .. }));
    }

    #[test]
    fn parse_rejects_short_content_data() {
        // Header claims 100 data bytes but only 3 follow.
        let mut buf = BytesMut::new();
        buf.put_u64_le(0);
        buf.put_u32_le(100);
        buf.put_slice(&[1, 2, 3]);
        let err = FileContentAsk::parse(&buf).unwrap_err();
        assert!(matches!(err, ProtoError::ShortFrame { .. }));
    }

    #[test]
    fn string_field_survives_multibyte_utf8() {
        let ask = FileStartAsk {
            name: "café-λ.png".into(),
            file_type: "image".into(),
            size: 10,
            md5: [0; MD5_LEN],
        };
        let bytes = ask.encode().unwrap();
        assert_eq!(FileStartAsk::parse(&bytes).unwrap(), ask);
    }
}
