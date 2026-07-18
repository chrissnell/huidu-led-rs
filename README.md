# huidu

Async Rust client for **Huidu full-color LED controller cards**, speaking the
Huidu SDK 2.0 binary TCP protocol and the HD2020 Gen6 realtime protocol.

## Status

**Design phase.** No implementation yet. The wire protocol is understood; the
API is designed and documented; code lands next.

- Design doc: [`DESIGN.md`](DESIGN.md)
- Reference implementation (Go, consulted for protocol facts only):
  [alparslanahmed/huidu-led](https://github.com/alparslanahmed/huidu-led)

Follow along or grep git history if you want to see the crate come up.

---

## Scope

Two crates in one workspace:

| Crate | Purpose |
|-------|---------|
| `huidu-proto` | Wire framing, SDK XML message types, HD2020 bitmap encoding. No I/O. Pure `bytes ↔ message` codec. |
| `huidu` | Async device client on `tokio`. Handshake, heartbeat, command dispatch, file upload, screen builder. |

Splitting the codec from the I/O layer means tests hit real bytes, not a
socket, and downstream users who want sync or embedded transports can depend on
`huidu-proto` alone.

---

## Target hardware

| Controller | Protocol | Confirmed on |
|------------|----------|--------------|
| HD-WF2 and other SDK 2.0 full-color cards | SDK 2.0 binary TCP (port 10001) | Reference implementation |
| HD2020 Gen6 (HD-E63 and similar) | HD2020 realtime bitmap | Reference implementation |

Protocol selection happens automatically at connect time based on the device's
probe response.

---

## Planned API

The examples below describe the *target* API. None of this code compiles yet.

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

### Upload a file with progress

```rust
use futures::StreamExt;

let mut progress = device.upload_file("/path/to/logo.jpg").await?;
while let Some(p) = progress.next().await {
    let p = p?;
    eprintln!("{}: {:.1}% ({}/{})", p.filename, p.percent, p.sent, p.total);
}
```

---

## Planned feature coverage

Parity with the Go reference implementation's SDK 2.0 command set.

- Device info (`GetDeviceInfo`)
- Screen builder (Screen → Program → Area → Item) with all 30+ transition
  effects for text, image, video, and clock items
- Brightness — manual, scheduled, sensor
- Screen on/off — immediate and scheduled
- Network — Ethernet (static/DHCP) and WiFi (AP + Station)
- Time sync — set time, timezone, DST
- File upload — chunked, MD5-verified, resume-capable, progress stream
- File management — list, delete
- Boot logo — get/set/clear
- TCP server config
- Heartbeat keep-alive
- Auto-detection of screen dimensions
- HD2020 Gen6 realtime text (fallback for older/simpler controllers)

Out of scope for v0: auto-reconnect (planned as a wrapper type), CLI binaries
(planned as a separate `huidu-cli` crate).

---

## Wire protocol summary

- **Framing:** `<len:u16le><cmd:u16le><payload>` for every packet.
- **Handshake:** 3 phases — transport version (0x2001), SDK version
  (`GetIFVersion` XML, returns session GUID), device info (`GetDeviceInfo`).
- **SDK envelope:** command 0x2003 wraps XML with a `<total_len:u32le,
  offset:u32le>` header, fragmenting XML payloads > 8000 bytes.
- **File upload:** three-phase state machine — `FileStartAsk` (0x8001) →
  chunked `FileContentAsk` (0x8003) → `FileEndAsk` (0x8005), with MD5
  verification and resume-on-partial support.
- **Heartbeat:** periodic 0x2005 pings keep the session alive; device replies
  with 0x2006.

Full protocol details in [`DESIGN.md §3`](DESIGN.md#3-wire-protocol-layer-huidu-proto).

---

## Development

Once code lands, standard workflow:

```bash
cargo test                          # codec goldens + mock device
cargo test --features hardware      # against real device at $HUIDU_TEST_ADDR
cargo doc --open                    # API docs
```

Golden fixtures live in `tests/fixtures/`, one `.bin` per captured message
with a `.txt` sidecar documenting provenance.

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

MIT. See [`LICENSE`](LICENSE) (once added).
