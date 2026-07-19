//! The bundled fixed-cell bitmap font used to rasterize HD2020 realtime text.
//!
//! `DESIGN.md §6` calls for a small pixel-aligned bitmap font blitted by hand,
//! rather than pulling in `ab_glyph` / a TTF just to draw 7px glyphs on a 64×32
//! panel. The glyph tables live in `font5x7.bin`, generated from the
//! human-readable ASCII art in `tools/gen_hd2020_font.py` and embedded here with
//! `include_bytes!`. There is no runtime font loading and no rasterizer.
//!
//! **Font provenance.** This is a hand-authored 5×7 monospace bitmap covering
//! printable ASCII, bundled under this crate's MIT license (see
//! `src/hd2020/FONT-LICENSE.md`). `DESIGN.md §6`/§11 item 4 propose bundling the
//! Plan 9 fixed 7×13 tables (BSD, the same source Go's `basicfont.Face7x13`
//! uses); swapping to those exact tables is a drop-in change — only the cell
//! dimensions and `font5x7.bin` contents move, and every consumer reads the
//! metrics through the constants below.

/// The embedded glyph blob: `GLYPH_COUNT` glyphs of `CELL_HEIGHT` bytes each,
/// in code-point order starting at [`FIRST_CODEPOINT`].
const FONT: &[u8] = include_bytes!("font5x7.bin");

/// Glyph cell width in pixels.
pub const CELL_WIDTH: usize = 5;
/// Glyph cell height in pixels.
pub const CELL_HEIGHT: usize = 7;

/// First code point with a glyph (space).
pub const FIRST_CODEPOINT: u32 = 0x20;
/// Last code point with a glyph (`~`).
pub const LAST_CODEPOINT: u32 = 0x7E;

/// Number of glyphs in the embedded font.
pub const GLYPH_COUNT: usize = (LAST_CODEPOINT - FIRST_CODEPOINT + 1) as usize;

/// Glyph substituted for any character outside the printable-ASCII range: a
/// hollow box, so missing characters are visible rather than silently blank.
const MISSING_GLYPH: [u8; CELL_HEIGHT] = [
    0b11111, //
    0b10001, //
    0b10001, //
    0b10001, //
    0b10001, //
    0b10001, //
    0b11111, //
];

/// The `CELL_HEIGHT` row bitmaps for `ch`. In each byte only the low
/// [`CELL_WIDTH`] bits are meaningful; bit `CELL_WIDTH - 1` is the leftmost
/// column. Characters without a glyph return [`MISSING_GLYPH`].
pub fn glyph_rows(ch: char) -> [u8; CELL_HEIGHT] {
    let cp = ch as u32;
    if !(FIRST_CODEPOINT..=LAST_CODEPOINT).contains(&cp) {
        return MISSING_GLYPH;
    }
    let start = (cp - FIRST_CODEPOINT) as usize * CELL_HEIGHT;
    let mut rows = [0u8; CELL_HEIGHT];
    rows.copy_from_slice(&FONT[start..start + CELL_HEIGHT]);
    rows
}

/// Whether `ch` has a dedicated glyph (vs. rendering as the missing-glyph box).
pub fn has_glyph(ch: char) -> bool {
    (FIRST_CODEPOINT..=LAST_CODEPOINT).contains(&(ch as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_has_one_row_byte_per_glyph_row() {
        assert_eq!(FONT.len(), GLYPH_COUNT * CELL_HEIGHT);
    }

    #[test]
    fn space_is_blank() {
        assert_eq!(glyph_rows(' '), [0u8; CELL_HEIGHT]);
    }

    #[test]
    fn capital_a_has_expected_top_and_crossbar() {
        // 'A': .###. on top, ##### crossbar in the middle row.
        let a = glyph_rows('A');
        assert_eq!(a[0] & 0b11111, 0b01110);
        assert_eq!(a[3] & 0b11111, 0b11111);
    }

    #[test]
    fn unknown_char_falls_back_to_box() {
        assert!(!has_glyph('€'));
        assert_eq!(glyph_rows('€'), MISSING_GLYPH);
    }

    #[test]
    fn every_printable_ascii_has_a_glyph() {
        for cp in FIRST_CODEPOINT..=LAST_CODEPOINT {
            let ch = char::from_u32(cp).unwrap();
            assert!(has_glyph(ch), "missing {ch:?}");
        }
    }
}
