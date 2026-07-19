# Plan: `huidu` screen builder & send

**Subsystem 6 of 8** (GRA-397). Reference: `DESIGN.md` §5, §12 (item 6).

**Goal:** Give the `huidu` crate the high-level content model — `Screen` /
`Program` / `Area` / `Item` — and the `send_screen` / `send_text` methods that
serialize that tree to SDK 2.0 program XML and push it to the device. Builds on
the transport/handshake (subsystem 4) and the `ProgramPush` envelope (subsystem
2), which already carries an opaque `<screen>` fragment; this subsystem is what
produces that fragment.

## Design constraints (DESIGN §5)

- **Owned-then-push, not mutation-through-pointer.** `area.add_text(...)`,
  `prog.push_area(area)`, `screen.push_program(prog)`. No `&mut` handles into a
  parent tree, no id-lookup API.
- **Config structs are `Default` + struct-update.** `TextConfig`, `ClockConfig`,
  `Color`, `Effect`. No `WithFoo` builder soup on configs, no caller-facing
  `Option<T>` the caller must unwrap.
- **One `Screen` type**, validated at `send_screen` time (DESIGN §11 q3). v0
  sends only SDK 2.0; HD2020 rejection lands with subsystem 8.

## Layout

```
crates/huidu/src/
├── screen/
│   ├── mod.rs      # Screen, Program, Area, Item, PlayControl, XML serialization
│   └── config.rs   # Color, Effect, HAlign, VAlign, TextConfig, ClockConfig
├── device.rs       # + send_screen, send_text
└── lib.rs          # re-export the screen surface
```

XML is written with `huidu_proto::sdk::xml::XmlWriter` — the same escaping-aware
writer every message body uses. huidu adds no XML dependency of its own. The one
primitive missing there — a text-node writer for `<string>content</string>` — is
added to `XmlWriter` (`text()`), since huidu-proto owns XML writing.

## Content model

```rust
pub struct Screen  { programs: Vec<Program> }
pub struct Program { name: String, play: PlayControl, areas: Vec<Area> }
pub struct Area    { x, y, width, height: u32, items: Vec<Item> }
pub enum   Item    { Text { text: String, config: TextConfig }, Clock { config: ClockConfig } }
pub enum   PlayControl { Count(u32), Duration(Duration) }   // default Count(1)
```

- `Screen::new` / `push_program` / `is_empty`.
- `Program::new(name)`; `with_duration_secs` / `with_play_count` chainable
  setters (on the tree node, not a config struct — matches the README target
  API); `push_area`.
- `Area::new(x, y, w, h)` and `Area::full_screen(w, h)`; `add_text`, `add_clock`.

## Config types (`config.rs`)

- `Color { r, g, b }` with `const` constructors and named constants (`RED`,
  `WHITE`, `CYAN`, …); serializes `#rrggbb`. Default `WHITE`.
- `Effect` — C-like enum of the SDK display effects with a `code()` and
  `is_continuous_scroll()` (drives the `singleLine` attribute). Default
  `Immediate`. Exact device codes are firmware-defined and undocumented publicly;
  we pick a stable, documented ordering (DESIGN §1: the Go API/codes are not
  carried over) and confirm against hardware in the Tier-3 pass.
- `HAlign` / `VAlign` → `left|center|right`, `top|middle|bottom`.
- `TextConfig` (color, font_name, font_size, bold/italic/underline, effect,
  out_effect, speed, hold_secs, h_align, v_align) and `ClockConfig` (show/color
  per field, formats, multi_line) — both `Default` + struct-update.

## Serialization

`Screen::to_program_xml(&self) -> Result<String, ProtoError>` emits the
`<screen>…</screen>` fragment (no XML declaration — `ProgramPush` injects it
verbatim into the request body). Node guids are **deterministic, index-derived**
(`program-0`, `area-0-1`, `text-0-1-2`) so the serializer is a pure function with
no clock/RNG — trivially golden-testable. Shape (ported from the Go reference,
consulted for protocol facts only):

```xml
<screen>
  <program type="normal" id="0" guid="program-0" name="welcome">
    <playControl count="1"/>
    <area guid="area-0-0" alpha="255">
      <rectangle x="0" y="0" width="128" height="64"/>
      <resources>
        <text guid="text-0-0-0" singleLine="false">
          <style align="center" valign="middle"/>
          <string>Welcome</string>
          <font name="Arial" size="16" color="#ffffff" bold="false" italic="false" underline="false"/>
          <effect in="9" inSpeed="4" out="0" outSpeed="4" duration="30"/>
        </text>
      </resources>
    </area>
  </program>
</screen>
```

Durations are deciseconds (`secs * 10`), matching the reference.

## `Device::send_screen` / `send_text`

```rust
pub async fn send_screen(&self, screen: &Screen) -> Result<()>;
pub async fn send_text(&self, text: &str, config: TextConfig) -> Result<()>;
```

`send_screen` guards on `ProtocolKind::Sdk2` (else `UnsupportedForProtocol`),
serializes the tree, wraps it with `ProgramPush::add(fragment)`, sends via the
existing `send_sdk` round-trip, and surfaces any device error from the reply.
`send_text` builds a one-program / one-full-screen-area / one-text `Screen` from
the device's cached `screen_width` × `screen_height` and delegates.

## Testing (DESIGN §10)

- **Unit (serialization):** pure `to_program_xml` assertions — element/attribute
  presence, escaping of text/attrs, `singleLine` toggling with continuous-scroll
  effects, deciseconds math, empty-screen fragment.
- **Tier-2 (`MockDevice`):** script `AddProgram` to capture the request, assert
  `send_screen` / `send_text` round-trip and that the embedded screen tree
  survives; assert a device-error reply surfaces as `Error::Proto(SdkError)`.
- One `XmlWriter::text` unit test in huidu-proto for the new primitive.
```
