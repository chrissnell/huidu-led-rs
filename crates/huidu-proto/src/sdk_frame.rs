//! The SDK XML fragmentation envelope carried inside 0x2003 / 0x2004 frames.

use crate::error::ProtoError;
use bytes::{BufMut, Bytes, BytesMut};

/// Bytes in the fragment header: total_len (u32) + offset (u32).
const HEADER_BYTES: usize = 8;

/// One SDK XML fragment: the envelope header plus this fragment's XML bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkFragment {
    pub total_len: u32,
    pub offset: u32,
    pub xml_chunk: Bytes,
}

impl SdkFragment {
    /// Parse the payload of an SDK frame into a fragment.
    pub fn parse(payload: &[u8]) -> Result<Self, ProtoError> {
        if payload.len() < HEADER_BYTES {
            return Err(ProtoError::ShortFrame {
                expected: HEADER_BYTES,
                got: payload.len(),
            });
        }
        let total_len = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let offset = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        let xml_chunk = Bytes::copy_from_slice(&payload[HEADER_BYTES..]);
        Ok(Self {
            total_len,
            offset,
            xml_chunk,
        })
    }

    /// Serialize this fragment into an SDK frame payload.
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(HEADER_BYTES + self.xml_chunk.len());
        buf.put_u32_le(self.total_len);
        buf.put_u32_le(self.offset);
        buf.put_slice(&self.xml_chunk);
        buf.freeze()
    }
}

/// Accumulates SDK fragments into a complete XML payload.
///
/// Assumes one in-flight message at a time, which holds because the transport
/// serializes every request/response through a single mutex (see `DESIGN.md §4.1`).
#[derive(Debug, Default)]
pub struct SdkReassembler {
    total_len: u32,
    buf: BytesMut,
}

impl SdkReassembler {
    /// A fresh reassembler with no message in progress.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one fragment. Returns `Some(xml)` once the whole message has arrived,
    /// then resets for the next message.
    pub fn push(&mut self, frag: SdkFragment) -> Result<Option<Bytes>, ProtoError> {
        if self.buf.is_empty() {
            self.total_len = frag.total_len;
        }
        let expected = self.buf.len() as u32;
        if frag.offset != expected {
            return Err(ProtoError::FragmentGap {
                expected,
                got: frag.offset,
            });
        }
        let end = frag.offset as usize + frag.xml_chunk.len();
        if end > frag.total_len as usize {
            return Err(ProtoError::FragmentOverflow {
                total: frag.total_len,
                offset: frag.offset,
                chunk: frag.xml_chunk.len(),
            });
        }
        self.buf.put_slice(&frag.xml_chunk);
        if self.buf.len() as u32 == self.total_len {
            let complete = std::mem::take(&mut self.buf).freeze();
            self.total_len = 0;
            return Ok(Some(complete));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_header_and_chunk() {
        // total_len = 5, offset = 0, chunk = "hello".
        let payload = [
            0x05, 0x00, 0x00, 0x00, // total_len
            0x00, 0x00, 0x00, 0x00, // offset
            b'h', b'e', b'l', b'l', b'o',
        ];
        let frag = SdkFragment::parse(&payload).unwrap();
        assert_eq!(frag.total_len, 5);
        assert_eq!(frag.offset, 0);
        assert_eq!(&frag.xml_chunk[..], b"hello");
    }

    #[test]
    fn parse_rejects_short_payload() {
        let err = SdkFragment::parse(&[0x00, 0x01, 0x02]).unwrap_err();
        assert!(matches!(err, ProtoError::ShortFrame { expected: 8, got: 3 }));
    }

    #[test]
    fn encode_parse_roundtrips() {
        let frag = SdkFragment {
            total_len: 12,
            offset: 4,
            xml_chunk: Bytes::from_static(b"world!"),
        };
        let bytes = frag.encode();
        assert_eq!(SdkFragment::parse(&bytes).unwrap(), frag);
    }

    #[test]
    fn reassembles_single_fragment() {
        let mut r = SdkReassembler::new();
        let frag = SdkFragment {
            total_len: 5,
            offset: 0,
            xml_chunk: Bytes::from_static(b"hello"),
        };
        assert_eq!(r.push(frag).unwrap().as_deref(), Some(&b"hello"[..]));
    }

    #[test]
    fn reassembles_two_fragments() {
        let mut r = SdkReassembler::new();
        let first = SdkFragment {
            total_len: 10,
            offset: 0,
            xml_chunk: Bytes::from_static(b"hello"),
        };
        let second = SdkFragment {
            total_len: 10,
            offset: 5,
            xml_chunk: Bytes::from_static(b"world"),
        };
        assert!(r.push(first).unwrap().is_none());
        assert_eq!(r.push(second).unwrap().as_deref(), Some(&b"helloworld"[..]));
    }

    #[test]
    fn rejects_out_of_order_fragment() {
        let mut r = SdkReassembler::new();
        let first = SdkFragment {
            total_len: 10,
            offset: 0,
            xml_chunk: Bytes::from_static(b"hello"),
        };
        r.push(first).unwrap();
        let bad = SdkFragment {
            total_len: 10,
            offset: 7, // expected 5
            xml_chunk: Bytes::from_static(b"xyz"),
        };
        let err = r.push(bad).unwrap_err();
        assert!(matches!(err, ProtoError::FragmentGap { expected: 5, got: 7 }));
    }

    #[test]
    fn rejects_overflowing_fragment() {
        let mut r = SdkReassembler::new();
        let frag = SdkFragment {
            total_len: 3,
            offset: 0,
            xml_chunk: Bytes::from_static(b"toolong"),
        };
        let err = r.push(frag).unwrap_err();
        assert!(matches!(
            err,
            ProtoError::FragmentOverflow {
                total: 3,
                offset: 0,
                chunk: 7
            }
        ));
    }

    #[test]
    fn resets_after_completion() {
        let mut r = SdkReassembler::new();
        let one = SdkFragment {
            total_len: 3,
            offset: 0,
            xml_chunk: Bytes::from_static(b"abc"),
        };
        assert_eq!(r.push(one).unwrap().as_deref(), Some(&b"abc"[..]));
        // A fresh message reuses the same reassembler starting at offset 0.
        let two = SdkFragment {
            total_len: 2,
            offset: 0,
            xml_chunk: Bytes::from_static(b"de"),
        };
        assert_eq!(r.push(two).unwrap().as_deref(), Some(&b"de"[..]));
    }
}
