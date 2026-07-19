//! Monochrome bitmap buffer, hand-written glyph blit, and the on-wire bitmap
//! payload carried by HD2020 realtime text frames.
//!
//! An HD2020 text panel is a 1-bit-per-pixel grid: each pixel is either lit or
//! dark. [`MonoBitmap`] holds that grid packed row-major, 8 pixels per byte,
//! most-significant bit leftmost — the packing HD2020 panels expect. [`render`]
//! rasterizes a string into a panel-sized bitmap using the bundled font
//! (`super::font`); there is no anti-aliasing, only set/clear pixels.

use super::font;
use crate::error::ProtoError;
use bytes::{BufMut, Bytes, BytesMut};

/// A packed 1-bpp bitmap: `width * height` pixels, row-major, each row padded to
/// a whole number of bytes. Within a byte, bit 7 is the leftmost pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonoBitmap {
    width: usize,
    height: usize,
    /// `stride * height` bytes.
    bits: Vec<u8>,
}

impl MonoBitmap {
    /// A blank (all-dark) bitmap of the given pixel dimensions.
    pub fn new(width: usize, height: usize) -> Self {
        let stride = Self::stride_for(width);
        Self {
            width,
            height,
            bits: vec![0u8; stride * height],
        }
    }

    /// Bytes per row for a bitmap `width` pixels wide.
    fn stride_for(width: usize) -> usize {
        width.div_ceil(8)
    }

    /// Panel width in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Panel height in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Bytes per packed row.
    pub fn stride(&self) -> usize {
        Self::stride_for(self.width)
    }

    /// The packed pixel bytes, row-major.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bits
    }

    /// Read one pixel. Out-of-bounds coordinates read as dark.
    pub fn get(&self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let byte = y * self.stride() + (x >> 3);
        (self.bits[byte] >> (7 - (x & 7))) & 1 == 1
    }

    /// Set one pixel. Out-of-bounds coordinates are ignored, so blits clip to
    /// the panel rather than panicking.
    pub fn set(&mut self, x: usize, y: usize, on: bool) {
        if x >= self.width || y >= self.height {
            return;
        }
        let stride = self.stride();
        let byte = y * stride + (x >> 3);
        let mask = 1u8 << (7 - (x & 7));
        if on {
            self.bits[byte] |= mask;
        } else {
            self.bits[byte] &= !mask;
        }
    }

    /// Blit one glyph with its top-left corner at `(ox, oy)`. Pixels off-panel
    /// are dropped.
    pub fn blit_char(&mut self, ch: char, ox: i32, oy: i32) {
        let rows = font::glyph_rows(ch);
        for (dy, row) in rows.iter().enumerate() {
            for dx in 0..font::CELL_WIDTH {
                if (row >> (font::CELL_WIDTH - 1 - dx)) & 1 == 1 {
                    let x = ox + dx as i32;
                    let y = oy + dy as i32;
                    if x >= 0 && y >= 0 {
                        self.set(x as usize, y as usize, true);
                    }
                }
            }
        }
    }

    /// Serialize to the HD2020 realtime bitmap payload:
    /// `[width:u16le][height:u16le][packed rows…]`.
    ///
    /// The two-byte dimension header is design-derived (see `hd2020::frame`);
    /// reconcile against the Go `hd2020_gen6_test.go` fixtures when available.
    pub fn to_payload(&self) -> Result<Bytes, ProtoError> {
        let w = u16::try_from(self.width)
            .map_err(|_| ProtoError::Hd2020(format!("bitmap width {} exceeds u16", self.width)))?;
        let h = u16::try_from(self.height).map_err(|_| {
            ProtoError::Hd2020(format!("bitmap height {} exceeds u16", self.height))
        })?;
        let mut buf = BytesMut::with_capacity(4 + self.bits.len());
        buf.put_u16_le(w);
        buf.put_u16_le(h);
        buf.put_slice(&self.bits);
        Ok(buf.freeze())
    }
}

/// How text is laid out into a panel by [`render`].
#[derive(Debug, Clone)]
pub struct TextLayout {
    /// Blank pixel columns inserted between adjacent glyphs.
    pub letter_spacing: usize,
    /// Pixel column the first glyph starts at.
    pub x_offset: usize,
    /// If set, the top pixel row for glyphs. When `None`, glyphs are centered
    /// vertically in the panel (top-aligned if the panel is shorter than a cell).
    pub y_offset: Option<usize>,
}

impl Default for TextLayout {
    fn default() -> Self {
        Self {
            letter_spacing: 1,
            x_offset: 0,
            y_offset: None,
        }
    }
}

impl TextLayout {
    /// Horizontal advance from one glyph's left edge to the next.
    fn advance(&self) -> usize {
        font::CELL_WIDTH + self.letter_spacing
    }
}

/// Pixel width the un-clipped rendering of `text` would occupy under `layout`
/// (glyph cells plus inter-glyph spacing, no trailing spacing).
pub fn measure(text: &str, layout: &TextLayout) -> usize {
    let n = text.chars().count();
    if n == 0 {
        return layout.x_offset;
    }
    layout.x_offset + n * font::CELL_WIDTH + (n - 1) * layout.letter_spacing
}

/// Rasterize `text` into a `width × height` panel bitmap. Glyphs past the right
/// edge (or below the bottom) are clipped.
pub fn render(text: &str, width: usize, height: usize, layout: &TextLayout) -> MonoBitmap {
    let mut bmp = MonoBitmap::new(width, height);
    let oy = match layout.y_offset {
        Some(y) => y as i32,
        None => (height as i32 - font::CELL_HEIGHT as i32) / 2,
    };
    let mut ox = layout.x_offset as i32;
    for ch in text.chars() {
        bmp.blit_char(ch, ox, oy);
        ox += layout.advance() as i32;
    }
    bmp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stride_rounds_up_to_bytes() {
        assert_eq!(MonoBitmap::new(1, 1).stride(), 1);
        assert_eq!(MonoBitmap::new(8, 1).stride(), 1);
        assert_eq!(MonoBitmap::new(9, 1).stride(), 2);
        assert_eq!(MonoBitmap::new(64, 1).stride(), 8);
    }

    #[test]
    fn set_get_roundtrips_and_packs_msb_first() {
        let mut b = MonoBitmap::new(8, 2);
        b.set(0, 0, true); // leftmost pixel -> bit 7
        b.set(7, 1, true); // rightmost pixel of row 1 -> bit 0
        assert!(b.get(0, 0));
        assert!(b.get(7, 1));
        assert!(!b.get(1, 0));
        assert_eq!(b.as_bytes(), &[0b1000_0000, 0b0000_0001]);
    }

    #[test]
    fn out_of_bounds_set_is_ignored() {
        let mut b = MonoBitmap::new(4, 4);
        b.set(10, 10, true); // no panic, no effect
        assert_eq!(b.as_bytes(), &[0u8; 4]);
    }

    #[test]
    fn blit_char_clips_negative_and_offscreen() {
        // Blit 'A' partly off the top-left; must not panic and must set at
        // least one on-panel pixel from the glyph body.
        let mut b = MonoBitmap::new(3, 3);
        b.blit_char('A', -1, -1);
        let any = (0..3).any(|y| (0..3).any(|x| b.get(x, y)));
        assert!(any);
    }

    #[test]
    fn render_places_first_glyph_column_at_x_offset() {
        // 'I' top row is .###. -> its leftmost lit column is x=1 within the cell.
        let layout = TextLayout {
            letter_spacing: 1,
            x_offset: 0,
            y_offset: Some(0),
        };
        let b = render("I", font::CELL_WIDTH, font::CELL_HEIGHT, &layout);
        assert!(!b.get(0, 0));
        assert!(b.get(1, 0));
        assert!(b.get(3, 0));
    }

    #[test]
    fn measure_accounts_for_spacing() {
        let layout = TextLayout::default(); // letter_spacing = 1
                                            // 3 glyphs: 3*5 + 2*1 = 17.
        assert_eq!(measure("abc", &layout), 17);
        assert_eq!(measure("", &layout), 0);
    }

    #[test]
    fn payload_prefixes_little_endian_dimensions() {
        let b = MonoBitmap::new(16, 7);
        let p = b.to_payload().unwrap();
        assert_eq!(&p[..4], &[16, 0, 7, 0]);
        assert_eq!(p.len(), 4 + b.stride() * 7);
    }
}
