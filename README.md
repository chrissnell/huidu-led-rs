# huidu

Async Rust client for **Huidu full-color LED controller cards**, speaking the
Huidu SDK 2.0 binary TCP protocol and the HD2020 Gen6 realtime protocol.

- Design doc: [`DESIGN.md`](DESIGN.md)
- Reference implementation (Go, consulted for protocol facts only):
  [alparslanahmed/huidu-led](https://github.com/alparslanahmed/huidu-led)

---

## Scope

Two crates in one workspace:

| Crate | Purpose |
|-------|---------|
| `huidu-proto` | Wire framing, SDK 2.0 XML message types, HD2020 bitmap encoding. No I/O. Pure `bytes ↔ message` codec. |
| `huidu` | Async device client on `tokio`. Handshake, heartbeat, command dispatch, screen builder. |

Splitting the codec from the I/O layer means tests hit real bytes, not a
socket, and downstream users who want sync or embedded transports can depend on
`huidu-proto` alone. The whole test suite runs without hardware.

---

## Target hardware

| Controller | Protocol |
|------------|----------|
| HD-WF2 and other SDK 2.0 full-color cards | SDK 2.0 binary TCP (port 10001) |
| HD2020 Gen6 (HD-E63 and similar) | HD2020 realtime bitmap |

Protocol selection happens automatically at connect time based on the device's
probe response.

---

## Usage

### Connect and send text

```rust
use huidu::{Device, DeviceConfig, TextConfig, Color, Effect};

#[tokio::main]
async fn main() -> Result<(), huidu::Error> {
    let device = Device::connect(
        "192.168.6.1:10001".parse()?,
        DeviceConfig::default(),
    ).await?;

    device.send_text("Hello, world!", TextConfig {
        color: Color::RED,
        font_size: 14,
        effect: Effect::LeftScrollLoop,
        speed: 4,
        ..Default::default()
    }).await?;

    Ok(())
}
```

`send_text` sizes the text area to the panel dimensions the device reports
during the handshake, so you don't pass a width or height.

### Build a multi-program screen

```rust
use huidu::{Screen, Program, Area, TextConfig, ClockConfig, Color, Effect};

let mut screen = Screen::new();

let mut welcome = Program::new("welcome").with_duration_secs(10);
let mut area = Area::full_screen(128, 64);
area.add_text("Welcome", TextConfig {
    color: Color::WHITE,
    font_size: 16,
    effect: Effect::FadeIn,
    ..Default::default()
});
welcome.push_area(area);
screen.push_program(welcome);

let mut clock = Program::new("clock");
let mut area = Area::full_screen(128, 64);
area.add_clock(ClockConfig {
    show_time: true,
    time_color: Color::CYAN,
    ..Default::default()
});
clock.push_area(area);
screen.push_program(clock);

device.send_screen(&screen).await?;
```

Configs are plain structs with `Default`. Use struct-update syntax to override
what you care about. No `WithFoo`/`WithBar` builder soup.

### Adjust the device

```rust
let info = device.get_device_info().await?;
println!("{} — {}x{}", info.model, info.screen_width, info.screen_height);

device.set_brightness_manual(60).await?;      // 0–100%
let files = device.list_files().await?;
```

---

## Feature coverage

Parity with the Go reference implementation's SDK 2.0 command set.

- Device info (`GetDeviceInfo`), including reported panel dimensions
- Screen builder (Screen → Program → Area → Item) with 12 entry/exit animation
  effects for text and clock items
- Brightness — manual, scheduled, and light-sensor modes
- Screen on/off scheduling (switch-time table)
- Network — Ethernet (static/DHCP) and WiFi (AP + Station)
- Time sync — set time, timezone, DST
- File management — list and delete
- Boot logo — get, set by name, clear
- TCP server config
- Heartbeat keep-alive
- HD2020 Gen6 realtime text (for older/simpler controllers)

Not included: auto-reconnect (a cancelled command poisons the connection and the
caller reconnects) and CLI binaries — both left for a later `huidu-cli` crate.

---

## Wire protocol summary

- **Framing:** `<len:u16le><cmd:u16le><payload>` for every packet.
- **Handshake:** 3 phases — transport version (0x2001), SDK version
  (`GetIFVersion` XML, returns session GUID), device info (`GetDeviceInfo`).
- **SDK envelope:** command 0x2003 wraps XML with a `<total_len:u32le,
  offset:u32le>` header, fragmenting XML payloads > 8000 bytes and reassembling
  them on receive.
- **File transfer:** frame codes for the three-phase upload state machine —
  `FileStartAsk` (0x8001) → `FileContentAsk` (0x8003) → `FileEndAsk` (0x8005) —
  are defined in `huidu-proto`.
- **Heartbeat:** periodic 0x2005 pings keep the session alive; device replies
  with 0x2006.

Full protocol details in [`DESIGN.md §3`](DESIGN.md#3-wire-protocol-layer-huidu-proto).

---

## Development

```bash
cargo test                          # codec goldens + mock-device flows
cargo doc --open                    # API docs
```

The test suite needs no hardware: `huidu-proto` tests decode golden byte
fixtures, and `huidu` tests drive a scripted in-process mock device. Golden
fixtures live in `crates/huidu-proto/tests/fixtures/`, one `.bin` per captured
message with a `.txt` sidecar documenting provenance.

---

## Credits

Protocol knowledge derives from the reverse-engineering work in
[alparslanahmed/huidu-led](https://github.com/alparslanahmed/huidu-led), which
in turn is based on the official Huidu C# SDK 2.0.10. This crate is a
from-scratch redesign that consults that project as a reference for the wire
protocol only — no code, API shape, or error semantics are carried over.

The bundled bitmap font for HD2020 text rendering is the Plan 9 fixed font
(BSD-licensed), the same source used by Go's `golang.org/x/image/font/basicfont`.

---

## License

MIT. See [`LICENSE`](LICENSE).
