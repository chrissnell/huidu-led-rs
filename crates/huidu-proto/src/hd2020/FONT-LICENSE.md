# Bundled bitmap font (`font5x7.bin`)

`font5x7.bin` is a hand-authored 5×7 monospace bitmap font covering printable
ASCII (`0x20`–`0x7E`). It was written from scratch for this crate — see the
source-of-truth ASCII art in `tools/gen_hd2020_font.py` — and carries no
third-party font data, so it is distributed under the project's own license with
no additional attribution requirement.

The project license is the BSD 2-Clause text in the repository root `LICENSE`.
(Note: `crates/huidu-proto/Cargo.toml` and `README.md` currently declare `MIT` —
a pre-existing inconsistency with the `LICENSE` file that predates this module
and should be reconciled project-wide.)

## Planned swap to the Plan 9 fixed font

`DESIGN.md §6` and §11 (item 4) propose bundling the Plan 9 fixed 7×13 bitmap
tables instead — the same BSD-licensed source that Go's
`golang.org/x/image/font/basicfont` (`Face7x13`) draws from. Those tables are
redistributable under the Plan 9 / Lucent Public License terms carried by the X
Consortium "misc-fixed" font, which is BSD-compatible.

If/when that swap happens:

1. Replace `font5x7.bin` with the Plan 9 7×13 glyph tables.
2. Update `CELL_WIDTH`/`CELL_HEIGHT` in `font.rs`.
3. Add the upstream BSD license text and attribution to this file.

The blit code in `bitmap.rs` reads the cell metrics through the `font.rs`
constants, so no other code changes.
