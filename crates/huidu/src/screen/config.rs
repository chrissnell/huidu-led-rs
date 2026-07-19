//! Content configuration for screen items (`DESIGN.md §5`).
//!
//! Every type here is a plain struct or C-like enum with a `Default`, meant to be
//! overridden with struct-update syntax:
//!
//! ```
//! use huidu::{TextConfig, Color, Effect};
//!
//! let cfg = TextConfig {
//!     color: Color::RED,
//!     font_size: 14,
//!     effect: Effect::LeftScrollLoop,
//!     speed: 4,
//!     ..Default::default()
//! };
//! ```
//!
//! No `WithFoo` builders and no caller-facing `Option<T>`: a field left alone
//! keeps its default, and there is nothing to unwrap.

/// A 24-bit RGB color. Serializes as `#rrggbb` — the form the SDK expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

impl Color {
    /// A color from its three channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Pure red.
    pub const RED: Color = Color::rgb(0xff, 0x00, 0x00);
    /// Pure green.
    pub const GREEN: Color = Color::rgb(0x00, 0xff, 0x00);
    /// Pure blue.
    pub const BLUE: Color = Color::rgb(0x00, 0x00, 0xff);
    /// White.
    pub const WHITE: Color = Color::rgb(0xff, 0xff, 0xff);
    /// Black.
    pub const BLACK: Color = Color::rgb(0x00, 0x00, 0x00);
    /// Yellow.
    pub const YELLOW: Color = Color::rgb(0xff, 0xff, 0x00);
    /// Cyan.
    pub const CYAN: Color = Color::rgb(0x00, 0xff, 0xff);
    /// Magenta.
    pub const MAGENTA: Color = Color::rgb(0xff, 0x00, 0xff);

    /// The `#rrggbb` string the SDK writes into `color` attributes.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::WHITE
    }
}

/// A text entry/exit animation. The numeric [`Effect::code`] is what the device
/// reads from the `<effect>` element's `in` / `out` attributes.
///
/// The exact firmware code table is not published; this ordering is the crate's
/// own stable mapping (`DESIGN.md §1`: the Go library's codes are not carried
/// over) and is confirmed against hardware in the Tier-3 test pass. The
/// continuous-scroll variants set the `singleLine` attribute — see
/// [`Effect::is_continuous_scroll`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u16)]
pub enum Effect {
    /// Draw at once, no animation.
    #[default]
    Immediate = 0,
    /// Slide in from the right, stopping in place (one pass).
    MoveLeft = 1,
    /// Slide in from the left, stopping in place (one pass).
    MoveRight = 2,
    /// Slide up into place (one pass).
    MoveUp = 3,
    /// Slide down into place (one pass).
    MoveDown = 4,
    /// Scroll leftward continuously (marquee).
    LeftScrollLoop = 5,
    /// Scroll rightward continuously.
    RightScrollLoop = 6,
    /// Scroll upward continuously.
    UpScrollLoop = 7,
    /// Scroll downward continuously.
    DownScrollLoop = 8,
    /// Fade in from black.
    FadeIn = 9,
    /// Fade out to black.
    FadeOut = 10,
    /// Flash on and off.
    Flicker = 11,
}

impl Effect {
    /// The integer the device reads from the `<effect>` `in` / `out` attribute.
    pub fn code(self) -> u16 {
        self as u16
    }

    /// Whether this is a continuous marquee scroll, which the firmware renders on
    /// a single non-wrapping line (`singleLine="true"`).
    pub fn is_continuous_scroll(self) -> bool {
        matches!(
            self,
            Effect::LeftScrollLoop
                | Effect::RightScrollLoop
                | Effect::UpScrollLoop
                | Effect::DownScrollLoop
        )
    }
}

/// Horizontal alignment of text within its area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HAlign {
    /// Align to the left edge.
    Left,
    /// Center horizontally.
    #[default]
    Center,
    /// Align to the right edge.
    Right,
}

impl HAlign {
    /// The `align` attribute string.
    pub fn as_str(self) -> &'static str {
        match self {
            HAlign::Left => "left",
            HAlign::Center => "center",
            HAlign::Right => "right",
        }
    }
}

/// Vertical alignment of text within its area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    /// Align to the top edge.
    Top,
    /// Center vertically.
    #[default]
    Middle,
    /// Align to the bottom edge.
    Bottom,
}

impl VAlign {
    /// The `valign` attribute string.
    pub fn as_str(self) -> &'static str {
        match self {
            VAlign::Top => "top",
            VAlign::Middle => "middle",
            VAlign::Bottom => "bottom",
        }
    }
}

/// How a piece of text looks and animates. Override with struct-update.
#[derive(Debug, Clone, PartialEq)]
pub struct TextConfig {
    /// Glyph color.
    pub color: Color,
    /// Font family name the device resolves (e.g. `"Arial"`).
    pub font_name: String,
    /// Font size in points.
    pub font_size: u32,
    /// Render bold.
    pub bold: bool,
    /// Render italic.
    pub italic: bool,
    /// Render underlined.
    pub underline: bool,
    /// Entry animation.
    pub effect: Effect,
    /// Exit animation.
    pub out_effect: Effect,
    /// Animation speed, `1`–`10` (higher is faster).
    pub speed: u32,
    /// Seconds the text holds on screen once it has entered.
    pub hold_secs: u32,
    /// Horizontal alignment.
    pub h_align: HAlign,
    /// Vertical alignment.
    pub v_align: VAlign,
}

impl Default for TextConfig {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            font_name: "Arial".to_string(),
            font_size: 12,
            bold: false,
            italic: false,
            underline: false,
            effect: Effect::Immediate,
            out_effect: Effect::Immediate,
            speed: 4,
            hold_secs: 3,
            h_align: HAlign::Center,
            v_align: VAlign::Middle,
        }
    }
}

/// A 12- or 24-hour clock face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeFormat {
    /// 24-hour (`13:05`).
    #[default]
    Hour24,
    /// 12-hour (`1:05`).
    Hour12,
}

impl TimeFormat {
    /// The `format` code the firmware reads on the `<time>` element.
    pub fn code(self) -> u32 {
        match self {
            TimeFormat::Hour24 => 0,
            TimeFormat::Hour12 => 1,
        }
    }
}

/// A digital-clock item. Each `show_*` toggles one line of the face; the
/// matching `*_color` tints it. Override with struct-update.
#[derive(Debug, Clone, PartialEq)]
pub struct ClockConfig {
    /// Show the time line.
    pub show_time: bool,
    /// Show the date line.
    pub show_date: bool,
    /// Show the weekday line.
    pub show_week: bool,
    /// Time color.
    pub time_color: Color,
    /// Date color.
    pub date_color: Color,
    /// Weekday color.
    pub week_color: Color,
    /// 12- or 24-hour time.
    pub time_format: TimeFormat,
    /// Lay the enabled lines out multi-line rather than on one row.
    pub multi_line: bool,
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self {
            show_time: true,
            show_date: false,
            show_week: false,
            time_color: Color::WHITE,
            date_color: Color::WHITE,
            week_color: Color::WHITE,
            time_format: TimeFormat::Hour24,
            multi_line: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_hex_is_lowercase_six_digits() {
        assert_eq!(Color::RED.to_hex(), "#ff0000");
        assert_eq!(Color::rgb(0x0a, 0xb0, 0x0c).to_hex(), "#0ab00c");
        assert_eq!(Color::default(), Color::WHITE);
    }

    #[test]
    fn effect_codes_and_scroll_classification() {
        assert_eq!(Effect::default(), Effect::Immediate);
        assert_eq!(Effect::Immediate.code(), 0);
        assert_eq!(Effect::LeftScrollLoop.code(), 5);
        assert!(Effect::LeftScrollLoop.is_continuous_scroll());
        assert!(!Effect::FadeIn.is_continuous_scroll());
        assert!(!Effect::MoveLeft.is_continuous_scroll());
    }

    #[test]
    fn defaults_are_sensible() {
        let t = TextConfig::default();
        assert_eq!(t.font_name, "Arial");
        assert_eq!(t.color, Color::WHITE);
        assert_eq!(t.h_align, HAlign::Center);
        let c = ClockConfig::default();
        assert!(c.show_time);
        assert!(!c.show_date);
        assert_eq!(c.time_format.code(), 0);
    }
}
