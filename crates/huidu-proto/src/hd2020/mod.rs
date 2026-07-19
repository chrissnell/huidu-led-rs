//! HD2020 Gen6 realtime protocol: framing, monochrome bitmap encoding, and a
//! bundled bitmap font — all I/O-free.
//!
//! HD2020 Gen6 controllers speak a realtime bitmap protocol that is distinct
//! from SDK 2.0 (`DESIGN.md §6`). Rather than uploading an XML program, the host
//! rasterizes content to a monochrome bitmap and pushes it straight to the
//! panel. This module keeps that protocol self-contained:
//!
//! - [`frame`] — the Gen6 frame envelope and [`Hd2020Cmd`] command set.
//! - [`bitmap`] — the [`MonoBitmap`] pixel buffer, hand-written glyph blit, and
//!   the on-wire bitmap payload.
//! - [`font`] — the bundled 5×7 bitmap font, embedded with `include_bytes!`.
//!
//! Several wire constants (start byte, command numbering, checksum, bitmap
//! header) are design-derived placeholders pending the Go `hd2020_gen6_test.go`
//! reference or a hardware capture; see [`frame`] for the full note.

pub mod bitmap;
pub mod font;
pub mod frame;

pub use bitmap::{measure, render, MonoBitmap, TextLayout};
pub use frame::{Hd2020Cmd, Hd2020Frame, START};

use crate::error::ProtoError;
use bytes::Bytes;

/// Rasterize `text` into a `width × height` panel and wrap it in a ready-to-send
/// [`Hd2020Cmd::RealtimeText`] frame — the common one-shot path.
pub fn realtime_text_frame(
    text: &str,
    width: usize,
    height: usize,
    layout: &TextLayout,
) -> Result<Bytes, ProtoError> {
    let payload = render(text, width, height, layout).to_payload()?;
    Hd2020Frame::new(Hd2020Cmd::RealtimeText, payload).encode()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_text_frame_roundtrips_to_a_bitmap() {
        let layout = TextLayout::default();
        let wire = realtime_text_frame("HI", 32, 8, &layout).unwrap();
        let (frame, used) = Hd2020Frame::decode(&wire).unwrap().unwrap();
        assert_eq!(used, wire.len());
        assert_eq!(frame.cmd, Hd2020Cmd::RealtimeText);
        // Payload begins with the little-endian 32×8 dimensions.
        assert_eq!(&frame.payload[..4], &[32, 0, 8, 0]);
        // Some pixels are lit — the text actually rendered.
        assert!(frame.payload[4..].iter().any(|&b| b != 0));
    }
}
