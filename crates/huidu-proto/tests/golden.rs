//! Golden-fixture round-trip tests: decode captured bytes, and re-encode to
//! confirm the codec reproduces them byte-for-byte.

use bytes::{Bytes, BytesMut};
use huidu_proto::{CmdCode, HuiduCodec, OwnedFrame, SdkFragment};
use std::path::Path;
use tokio_util::codec::{Decoder, Encoder};

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn heartbeat_golden_decodes() {
    let mut codec = HuiduCodec;
    let mut buf = BytesMut::from(&fixture("heartbeat.bin")[..]);
    let frame = codec.decode(&mut buf).unwrap().unwrap();
    assert_eq!(frame.cmd, CmdCode::Heartbeat);
    assert!(frame.payload.is_empty());
    assert!(buf.is_empty());
}

#[test]
fn heartbeat_golden_reencodes() {
    let golden = fixture("heartbeat.bin");
    let mut codec = HuiduCodec;
    let mut out = BytesMut::new();
    codec
        .encode(OwnedFrame::new(CmdCode::Heartbeat, Bytes::new()), &mut out)
        .unwrap();
    assert_eq!(&out[..], &golden[..]);
}

#[test]
fn sdk_hello_golden_decodes_and_parses() {
    let mut codec = HuiduCodec;
    let mut buf = BytesMut::from(&fixture("sdk_hello.bin")[..]);
    let frame = codec.decode(&mut buf).unwrap().unwrap();
    assert_eq!(frame.cmd, CmdCode::SdkCmd);
    let frag = SdkFragment::parse(&frame.payload).unwrap();
    assert_eq!(frag.total_len, 5);
    assert_eq!(frag.offset, 0);
    assert_eq!(&frag.xml_chunk[..], b"hello");
}
