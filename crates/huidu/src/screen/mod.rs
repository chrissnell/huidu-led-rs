//! The high-level content model: `Screen` → `Program` → `Area` → `Item`, and
//! its serialization to SDK 2.0 program XML (`DESIGN.md §5`).
//!
//! The tree is built with the **owned-then-push** pattern — you construct a node,
//! fill it, and hand it to its parent — rather than the Go library's
//! mutation-through-returned-pointer API, which fights the borrow checker:
//!
//! ```
//! use huidu::{Screen, Program, Area, TextConfig, Color, Effect};
//!
//! let mut screen = Screen::new();
//! let mut prog = Program::new("hello");
//! let mut area = Area::full_screen(128, 64);
//! area.add_text("Hello World!", TextConfig {
//!     color: Color::RED,
//!     effect: Effect::LeftScrollLoop,
//!     ..Default::default()
//! });
//! prog.push_area(area);
//! screen.push_program(prog);
//! ```
//!
//! [`Screen::to_program_xml`] renders the tree to the `<screen>…</screen>`
//! fragment that `AddProgram` carries. Node guids are derived from tree position
//! (`program-0`, `area-0-1`, `text-0-1-2`), so serialization is a pure function
//! with no clock or randomness — it round-trips byte-for-byte in tests.

mod config;

pub use config::{ClockConfig, Color, Effect, HAlign, TextConfig, TimeFormat, VAlign};

use huidu_proto::sdk::xml::XmlWriter;
use huidu_proto::ProtoError;
use std::time::Duration;

/// A full display: an ordered set of programs the device cycles through.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Screen {
    programs: Vec<Program>,
}

impl Screen {
    /// An empty screen. Sending it clears all programs from the device.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a program. Programs play in push order.
    pub fn push_program(&mut self, program: Program) {
        self.programs.push(program);
    }

    /// Whether the screen has no programs.
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    /// The programs, in play order.
    pub fn programs(&self) -> &[Program] {
        &self.programs
    }

    /// Serialize to the `<screen>…</screen>` fragment `AddProgram` carries. No
    /// XML declaration — the request envelope injects the fragment verbatim.
    pub fn to_program_xml(&self) -> Result<String, ProtoError> {
        let mut x = XmlWriter::new();
        x.open("screen", &[])?;
        for (pi, program) in self.programs.iter().enumerate() {
            program.write_xml(&mut x, pi)?;
        }
        x.close("screen")?;
        String::from_utf8(x.into_bytes()).map_err(|e| ProtoError::Xml(e.to_string()))
    }
}

/// How long a program stays up before the device advances to the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayControl {
    /// Play the program's content through `n` times, then advance.
    Count(u32),
    /// Hold the program for a fixed wall-clock duration.
    Duration(Duration),
}

impl Default for PlayControl {
    fn default() -> Self {
        PlayControl::Count(1)
    }
}

/// One program in a [`Screen`]: a named collection of areas with play timing.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// Human-facing program name (echoed to the device).
    pub name: String,
    /// When to advance to the next program.
    pub play: PlayControl,
    areas: Vec<Area>,
}

impl Program {
    /// A new program with the given name, played once through (the default
    /// [`PlayControl`]).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            play: PlayControl::default(),
            areas: Vec::new(),
        }
    }

    /// Hold this program for a fixed number of seconds, chainable at construction.
    pub fn with_duration_secs(mut self, secs: u64) -> Self {
        self.play = PlayControl::Duration(Duration::from_secs(secs));
        self
    }

    /// Play this program's content through `count` times, chainable at
    /// construction.
    pub fn with_play_count(mut self, count: u32) -> Self {
        self.play = PlayControl::Count(count);
        self
    }

    /// Append an area. Areas render stacked in push order.
    pub fn push_area(&mut self, area: Area) {
        self.areas.push(area);
    }

    /// The areas, in render order.
    pub fn areas(&self) -> &[Area] {
        &self.areas
    }

    fn write_xml(&self, x: &mut XmlWriter, pi: usize) -> Result<(), ProtoError> {
        let id = pi.to_string();
        let guid = format!("program-{pi}");
        x.open(
            "program",
            &[
                ("type", "normal"),
                ("id", &id),
                ("guid", &guid),
                ("name", &self.name),
            ],
        )?;
        match self.play {
            PlayControl::Count(n) => {
                x.empty("playControl", &[("count", &n.to_string())])?;
            }
            PlayControl::Duration(d) => {
                // Deciseconds, matching the SDK's tenths-of-a-second timing unit.
                let ds = (d.as_secs() * 10).to_string();
                x.empty("playControl", &[("duration", &ds)])?;
            }
        }
        for (ai, area) in self.areas.iter().enumerate() {
            area.write_xml(x, pi, ai)?;
        }
        x.close("program")?;
        Ok(())
    }
}

/// A rectangular region of a program that holds content items.
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    /// Left edge in pixels.
    pub x: u32,
    /// Top edge in pixels.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    items: Vec<Item>,
}

impl Area {
    /// An area at `(x, y)` of the given size.
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            items: Vec::new(),
        }
    }

    /// An area covering the whole panel (`0, 0, width, height`).
    pub fn full_screen(width: u32, height: u32) -> Self {
        Self::new(0, 0, width, height)
    }

    /// Add a text item.
    pub fn add_text(&mut self, text: impl Into<String>, config: TextConfig) {
        self.items.push(Item::Text {
            text: text.into(),
            config,
        });
    }

    /// Add a digital-clock item.
    pub fn add_clock(&mut self, config: ClockConfig) {
        self.items.push(Item::Clock { config });
    }

    /// The items in this area, in render order.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    fn write_xml(&self, x: &mut XmlWriter, pi: usize, ai: usize) -> Result<(), ProtoError> {
        let guid = format!("area-{pi}-{ai}");
        x.open("area", &[("guid", &guid), ("alpha", "255")])?;
        x.empty(
            "rectangle",
            &[
                ("x", &self.x.to_string()),
                ("y", &self.y.to_string()),
                ("width", &self.width.to_string()),
                ("height", &self.height.to_string()),
            ],
        )?;
        x.open("resources", &[])?;
        for (ii, item) in self.items.iter().enumerate() {
            item.write_xml(x, pi, ai, ii)?;
        }
        x.close("resources")?;
        x.close("area")?;
        Ok(())
    }
}

/// A single content element inside an [`Area`].
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Styled, animated text.
    Text {
        /// The displayed string.
        text: String,
        /// Font, color, effect, and layout.
        config: TextConfig,
    },
    /// A digital clock.
    Clock {
        /// Which lines to show and how to tint them.
        config: ClockConfig,
    },
}

impl Item {
    fn write_xml(
        &self,
        x: &mut XmlWriter,
        pi: usize,
        ai: usize,
        ii: usize,
    ) -> Result<(), ProtoError> {
        match self {
            Item::Text { text, config } => write_text(x, pi, ai, ii, text, config),
            Item::Clock { config } => write_clock(x, pi, ai, ii, config),
        }
    }
}

fn write_text(
    x: &mut XmlWriter,
    pi: usize,
    ai: usize,
    ii: usize,
    text: &str,
    c: &TextConfig,
) -> Result<(), ProtoError> {
    let guid = format!("text-{pi}-{ai}-{ii}");
    let single_line = bool_str(c.effect.is_continuous_scroll());
    x.open("text", &[("guid", &guid), ("singleLine", single_line)])?;
    x.empty(
        "style",
        &[
            ("align", c.h_align.as_str()),
            ("valign", c.v_align.as_str()),
        ],
    )?;
    x.open("string", &[])?;
    x.text(text)?;
    x.close("string")?;
    x.empty(
        "font",
        &[
            ("name", &c.font_name),
            ("size", &c.font_size.to_string()),
            ("color", &c.color.to_hex()),
            ("bold", bool_str(c.bold)),
            ("italic", bool_str(c.italic)),
            ("underline", bool_str(c.underline)),
        ],
    )?;
    // Effect timing is in deciseconds, matching the SDK unit.
    let hold_ds = (c.hold_secs * 10).to_string();
    x.empty(
        "effect",
        &[
            ("in", &c.effect.code().to_string()),
            ("inSpeed", &c.speed.to_string()),
            ("out", &c.out_effect.code().to_string()),
            ("outSpeed", &c.speed.to_string()),
            ("duration", &hold_ds),
        ],
    )?;
    x.close("text")?;
    Ok(())
}

fn write_clock(
    x: &mut XmlWriter,
    pi: usize,
    ai: usize,
    ii: usize,
    c: &ClockConfig,
) -> Result<(), ProtoError> {
    let guid = format!("clock-{pi}-{ai}-{ii}");
    x.open(
        "clock",
        &[
            ("guid", &guid),
            ("type", "0"),
            ("multiLine", bool_str(c.multi_line)),
        ],
    )?;
    x.empty(
        "time",
        &[
            ("display", bool_str(c.show_time)),
            ("format", &c.time_format.code().to_string()),
            ("color", &c.time_color.to_hex()),
        ],
    )?;
    x.empty(
        "date",
        &[
            ("display", bool_str(c.show_date)),
            ("color", &c.date_color.to_hex()),
        ],
    )?;
    x.empty(
        "week",
        &[
            ("display", bool_str(c.show_week)),
            ("color", &c.week_color.to_hex()),
        ],
    )?;
    x.close("clock")?;
    Ok(())
}

/// The lowercase boolean strings the firmware expects in attributes.
fn bool_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_program(effect: Effect) -> Screen {
        let mut screen = Screen::new();
        let mut prog = Program::new("welcome");
        let mut area = Area::full_screen(128, 64);
        area.add_text(
            "Hi",
            TextConfig {
                color: Color::RED,
                font_size: 14,
                effect,
                speed: 4,
                hold_secs: 3,
                ..Default::default()
            },
        );
        prog.push_area(area);
        screen.push_program(prog);
        screen
    }

    #[test]
    fn empty_screen_serializes_to_empty_screen_element() {
        let xml = Screen::new().to_program_xml().unwrap();
        assert_eq!(xml, "<screen></screen>");
    }

    #[test]
    fn text_program_has_expected_shape() {
        let xml = text_program(Effect::Immediate).to_program_xml().unwrap();
        assert!(xml.starts_with(
            "<screen><program type=\"normal\" id=\"0\" guid=\"program-0\" name=\"welcome\">"
        ));
        assert!(xml.contains("<playControl count=\"1\"/>"));
        assert!(xml.contains("<rectangle x=\"0\" y=\"0\" width=\"128\" height=\"64\"/>"));
        assert!(xml.contains("<string>Hi</string>"));
        assert!(xml.contains("color=\"#ff0000\""));
        assert!(xml.contains("size=\"14\""));
        assert!(xml
            .contains("<effect in=\"0\" inSpeed=\"4\" out=\"0\" outSpeed=\"4\" duration=\"30\"/>"));
        assert!(xml.ends_with("</program></screen>"));
    }

    #[test]
    fn continuous_scroll_sets_single_line() {
        let scroll = text_program(Effect::LeftScrollLoop)
            .to_program_xml()
            .unwrap();
        assert!(scroll.contains("singleLine=\"true\""));
        assert!(scroll.contains("<effect in=\"5\""));
        let still = text_program(Effect::Immediate).to_program_xml().unwrap();
        assert!(still.contains("singleLine=\"false\""));
    }

    #[test]
    fn text_content_and_attrs_are_escaped() {
        let mut screen = Screen::new();
        let mut prog = Program::new("a & b");
        let mut area = Area::full_screen(64, 32);
        area.add_text("<x> & \"y\"", TextConfig::default());
        prog.push_area(area);
        screen.push_program(prog);
        let xml = screen.to_program_xml().unwrap();
        assert!(xml.contains("<string>&lt;x&gt; &amp; &quot;y&quot;</string>"));
        assert!(xml.contains("name=\"a &amp; b\""));
    }

    #[test]
    fn duration_playcontrol_uses_deciseconds() {
        let mut screen = Screen::new();
        screen.push_program(Program::new("p").with_duration_secs(10));
        let xml = screen.to_program_xml().unwrap();
        assert!(xml.contains("<playControl duration=\"100\"/>"));
    }

    #[test]
    fn clock_item_emits_lines() {
        let mut screen = Screen::new();
        let mut prog = Program::new("clock");
        let mut area = Area::full_screen(128, 64);
        area.add_clock(ClockConfig {
            show_time: true,
            show_date: true,
            time_color: Color::CYAN,
            ..Default::default()
        });
        prog.push_area(area);
        screen.push_program(prog);
        let xml = screen.to_program_xml().unwrap();
        assert!(xml.contains("<clock guid=\"clock-0-0-0\" type=\"0\" multiLine=\"false\">"));
        assert!(xml.contains("<time display=\"true\" format=\"0\" color=\"#00ffff\"/>"));
        assert!(xml.contains("<date display=\"true\" color=\"#ffffff\"/>"));
        assert!(xml.contains("<week display=\"false\""));
    }

    #[test]
    fn multiple_programs_and_areas_get_positional_guids() {
        let mut screen = Screen::new();
        let mut p0 = Program::new("p0");
        let mut a0 = Area::new(0, 0, 64, 32);
        a0.add_text("one", TextConfig::default());
        p0.push_area(a0);
        screen.push_program(p0);
        let mut p1 = Program::new("p1");
        let mut a1 = Area::new(0, 0, 64, 32);
        a1.add_text("two", TextConfig::default());
        p1.push_area(a1);
        screen.push_program(p1);
        let xml = screen.to_program_xml().unwrap();
        assert!(xml.contains("guid=\"program-0\""));
        assert!(xml.contains("guid=\"program-1\""));
        assert!(xml.contains("guid=\"text-1-0-0\""));
    }
}
