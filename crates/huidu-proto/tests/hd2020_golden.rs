//! Golden-fixture round-trip tests for the HD2020 Gen6 protocol: decode the
//! captured bytes, and re-encode to confirm the codec reproduces them exactly.
//!
//! The fixtures are design-derived (see each `.txt` sidecar) — they pin the
//! current wire format so an accidental change is caught, and become real
//! captures once an HD2020 Gen6 device or the Go reference is available.

use huidu_proto::hd2020::{realtime_text_frame, Hd2020Cmd, Hd2020Frame, TextLayout};
use std::path::Path;

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn brightness_golden_decodes() {
    let (frame, used) = Hd2020Frame::decode(&fixture("hd2020_brightness.bin"))
        .unwrap()
        .unwrap();
    assert_eq!(frame.cmd, Hd2020Cmd::Brightness);
    assert_eq!(&frame.payload[..], &[0x80]);
    assert_eq!(used, 6);
}

#[test]
fn brightness_golden_reencodes() {
    let golden = fixture("hd2020_brightness.bin");
    let bytes = Hd2020Frame::new(Hd2020Cmd::Brightness, vec![0x80u8])
        .encode()
        .unwrap();
    assert_eq!(&bytes[..], &golden[..]);
}

#[test]
fn realtime_hi_golden_decodes_to_a_32x8_bitmap() {
    let (frame, used) = Hd2020Frame::decode(&fixture("hd2020_realtime_hi.bin"))
        .unwrap()
        .unwrap();
    assert_eq!(frame.cmd, Hd2020Cmd::RealtimeText);
    assert_eq!(used, fixture("hd2020_realtime_hi.bin").len());
    // 4-byte header: width=32, height=8, then one row of packed pixels.
    assert_eq!(&frame.payload[..4], &[0x20, 0x00, 0x08, 0x00]);
    assert_eq!(&frame.payload[4..8], &[0x89, 0xc0, 0x00, 0x00]);
}

#[test]
fn realtime_hi_golden_reencodes() {
    let golden = fixture("hd2020_realtime_hi.bin");
    let bytes = realtime_text_frame("HI", 32, 8, &TextLayout::default()).unwrap();
    assert_eq!(&bytes[..], &golden[..]);
}
