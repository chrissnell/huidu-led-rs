# `huidu` — Rust rewrite design

Reference implementation being consulted for protocol facts only:
`../huidu-led/` (Go, alparslanahmed/huidu-led). Its API shape, error strings, and
idioms are not being carried over.

---

## 1. Goals & non-goals

### Goals

- Functional parity with the Go library's SDK 2.0 command set (24 XML methods,
  file upload, HD2020 Gen6 realtime + program bitmap modes).
- Idiomatic async Rust: `tokio` at the boundary, no blocking calls in the hot
  path.
- Clean separation between wire protocol (bytes on the socket) and SDK semantics
  (commands, XML, business logic). Testable without a device.
- Public API that survives the borrow checker without `Rc<RefCell<_>>`.
- Every error a caller might reasonably match on is a typed enum variant.

### Non-goals (v0)

- Blocking/sync API. Users needing sync can `block_on`.
- Transports other than raw TCP.
- Higher-level "campaign management" abstractions.
- CLI binaries — add later as a separate `huidu-cli` crate.
- Faithful reproduction of the Go `Screen → Program → Area → Item` API
  (mutation-through-returned-pointers). Rust wants a different shape here; see §5.
- Auto-reconnect. Add later as a wrapper type once the base API is proven.

---

## 2. Crate layout

Single workspace, two crates:

```
huidu-led-rs/
├── Cargo.toml            # workspace
├── crates/
│   ├── huidu-proto/      # wire protocol, XML types, codec — no I/O
│   └── huidu/            # async device client, tokio, high-level API
└── tests/
    └── fixtures/         # captured device traffic, golden bytes
```

**Why split.** `huidu-proto` is I/O-agnostic: pure `bytes → parsed message` and
`message → bytes`. Three concrete benefits:

1. Golden tests hit the codec directly, not through a socket.
2. Anyone wanting sync or embedded I/O can depend on `huidu-proto` alone.
3. Async concerns (`tokio`, `Stream`, cancellation) stay contained in `huidu`.

The Go project puts everything into one flat package. Fine for a first cut,
painful to test. We do not repeat that.

---

## 3. Wire protocol layer (`huidu-proto`)

### 3.1 Raw frames

Every TCP packet is `<len:u16le><cmd:u16le><payload>`. The command byte tells us
how to interpret the payload.

```rust
// crates/huidu-proto/src/frame.rs

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdCode {
    VersionAsk       = 0x2001,
    VersionReply     = 0x2002,
    SdkCmd           = 0x2003,
    SdkReply         = 0x2004,
    Heartbeat        = 0x2005,
    HeartbeatReply   = 0x2006,
    FileStartAsk     = 0x8001,
    FileStartReply   = 0x8002,
    FileContentAsk   = 0x8003,
    FileContentReply = 0x8004,
    FileEndAsk       = 0x8005,
    FileEndReply     = 0x8006,
    // HD2020 Gen6 command codes are added here — see §6.
}

pub struct RawFrame<'a> {
    pub cmd: CmdCode,
    pub payload: &'a [u8],
}
```

### 3.2 Codec

`tokio_util::codec::{Decoder, Encoder}` implementations for `RawFrame`. The
decoder reads the 2-byte length prefix, waits for the full frame, hands out an
owned frame value. The caller ends up with a `Framed<TcpStream, HuiduCodec>`
that yields frames.

```rust
// crates/huidu-proto/src/codec.rs

pub struct HuiduCodec;

impl Decoder for HuiduCodec {
    type Item = OwnedFrame;      // owned so Item has no lifetime
    type Error = ProtoError;
    // ...
}
```

### 3.3 SDK envelope (fragmentation)

SDK commands (0x2003 / 0x2004) add two `u32le` fields — total XML length and
XML fragment offset — for fragmentation of XML > 8000 bytes. This lives *above*
the raw frame codec.

```rust
// crates/huidu-proto/src/sdk_frame.rs

pub struct SdkFragment {
    pub total_len: u32,
    pub offset: u32,
    pub xml_chunk: Bytes,
}

pub struct SdkReassembler {
    // holds partial XML by (guid, method) until all fragments arrive
}
```

The reassembler owns the buffer-until-complete state. The Go code inlines this
in the request/response loop; we lift it into its own type with focused tests.

### 3.4 XML request/response types

`quick-xml` with `serde` derives. One module per method group inside
`huidu-proto::sdk::messages`:

- `device_info`
- `network` (ethernet, wifi)
- `program` (add/update/delete/get program, screen tree)
- `time`
- `luminance`
- `switch_time`
- `files` (list, delete)
- `boot_logo`
- `server` (TCP server config)

```rust
// crates/huidu-proto/src/sdk/messages/device_info.rs

#[derive(Debug, Deserialize)]
#[serde(rename = "out")]
pub struct DeviceInfoOut {
    #[serde(rename = "@result")]
    pub result: SdkResult,
    pub device: DeviceInfoBody,
}
```

We do not build XML by string concatenation the way the Go code does.
`quick-xml`'s writer is not much more code and it gets escaping right.

---

## 4. Async I/O & lifecycle (`huidu`)

### 4.1 Connection

```rust
pub struct Device {
    inner: Arc<DeviceInner>,
}

struct DeviceInner {
    frames: tokio::sync::Mutex<Framed<TcpStream, HuiduCodec>>,
    guid: OnceCell<Uuid>,          // set during handshake
    info: OnceCell<DeviceInfo>,    // set during handshake
    config: DeviceConfig,
    heartbeat_task: Mutex<Option<JoinHandle<()>>>,
    protocol: OnceCell<ProtocolKind>,
    poisoned: AtomicBool,
}

impl Device {
    pub async fn connect(addr: SocketAddr, config: DeviceConfig) -> Result<Self>;
    pub async fn close(self) -> Result<()>;
}
```

**Why `Arc<Inner>` + `Mutex<Framed<_>>`.** The transport is a single I/O
resource; parallel commands must serialize. Put the whole `Framed` behind a
`tokio::sync::Mutex` and let each command borrow it for the duration of a
request/response round-trip. This is exactly what the Go code does with
`writeMu` + `mu`, only cleaner because we're not fighting the language.

We do *not* try to run reads and writes concurrently on the same connection.
The protocol is strict request/response with heartbeat interleaving; a
multiplexer isn't justified in v0. Heartbeat serialises through the same mutex.

### 4.2 Handshake

Three-phase, spelled out linearly in `Device::connect`:

1. Send `VersionAsk`, await `VersionReply`.
2. Send `GetIFVersion` SDK cmd, await reply, extract session GUID.
3. Send `GetDeviceInfo` SDK cmd, await reply, cache `DeviceInfo`.

If any phase fails, close the socket and return an error tagged with the
failing phase. No auto-reconnect.

### 4.3 Heartbeat

Background `tokio::spawn` task started at end of `connect`, cancelled on
`close`. Owns a weak reference to `Inner` so it doesn't keep the device alive.

```rust
async fn heartbeat_loop(inner: Weak<DeviceInner>, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        let Some(inner) = inner.upgrade() else { return };
        if inner.send_heartbeat().await.is_err() {
            inner.poisoned.store(true, Ordering::SeqCst);
            return;
        }
    }
}
```

### 4.4 Cancellation

Every command takes `&self` and looks cancel-safe, but is not truly cancel-safe
mid-round-trip: if the caller drops the future between sending the request and
reading the reply, the reply is still on the wire and the connection is no
longer at a message boundary.

v0 rule: if a command's read future is cancelled, we set `poisoned = true` and
every subsequent command returns `Err(Poisoned)`. Callers reconnect. This is
honest and cheap.

v1 upgrade path (see §11): dedicated reader task that correlates replies to
requests by GUID + method, so cancelled requesters just lose their reply.

---

## 5. High-level API: screen builder

The Go API returns `*Program` from `screen.AddProgram(...)`, mutates it in
place, then returns `*Area` from `prog.AddArea(...)`, etc. This pattern fights
Rust's borrow checker: two live `&mut Program` handles into the same `Screen`
is a non-starter.

**Choice: owned-then-push.** Reads best, no lifetime gymnastics, no id lookups.

```rust
let mut screen = Screen::new();

let mut prog = Program::new("hello");
let mut area = Area::full_screen(width, height);
area.add_text("Hello World!", TextConfig {
    color: Color::RED,
    font_size: 14,
    effect: Effect::LeftScrollLoop,
    speed: 4,
    ..Default::default()
});
prog.push_area(area);
screen.push_program(prog);

device.send_screen(&screen).await?;
```

Configs use `Default` + struct-update syntax — no `WithFoo`/`WithBar` builder
soup, no `Option<T>` fields the caller has to unwrap. `TextConfig::default()`
gives you sensible defaults; you override what you care about.

**Rejected: id-handle API** (`ProgramId`). Reads worse and gives no real safety
benefit for a tree that's built once and serialized.

---

## 6. HD2020 Gen6 branch

This is a separate wire protocol on the same TCP port, detected from the probe
response. It does not share framing with SDK 2.0. Isolate cleanly:

```
crates/huidu-proto/src/
├── sdk2/          # 0x2001–0x2006, 0x8001–0x8006, XML
└── hd2020/        # HD2020 Gen6 framing, bitmap encoding
```

`Device::connect` probes, decides which protocol module handles the socket, and
dispatches. Commands that only exist on one protocol return
`Err(UnsupportedForProtocol)` on the other. This is honest about what the
device supports; the Go code silently misbehaves in some of these cases.

**Font rendering** for HD2020 text bitmaps: bundle a small pixel-aligned bitmap
font as `include_bytes!(...)` and rasterize with hand-written blit code.
Rationale: `ab_glyph` and TTFs are overkill for 7px glyphs on a 64×32 panel;
the Go code uses `basicfont.Face7x13` for exactly this reason. Port the Plan9
bitmap tables directly (BSD-licensed, same source `basicfont` uses).

---

## 7. Error model

Single top-level error enum per crate. `thiserror`.

```rust
// crates/huidu-proto/src/error.rs

#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("frame too short: expected {expected} bytes, got {got}")]
    ShortFrame { expected: usize, got: usize },

    #[error("unknown command code: 0x{0:04x}")]
    UnknownCmd(u16),

    #[error("XML parse error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("device returned error for {method}: {result:?}")]
    SdkError { method: String, result: SdkResult },

    #[error("HD2020 bitmap encoding failed: {0}")]
    Hd2020(String),
}
```

```rust
// crates/huidu/src/error.rs

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("protocol error: {0}")]
    Proto(#[from] huidu_proto::ProtoError),

    #[error("not connected")]
    NotConnected,

    #[error("connection poisoned by cancelled command")]
    Poisoned,

    #[error("handshake failed at phase {phase}: {source}")]
    Handshake { phase: u8, source: Box<Error> },

    #[error("operation not supported by {0:?} protocol")]
    UnsupportedForProtocol(ProtocolKind),

    #[error("timeout after {0:?}")]
    Timeout(Duration),
}
```

No `Box<dyn Error>` in the public API. Every error a caller might match on is
an enum variant.

---

## 8. File upload

`Device::upload_file(&self, path) -> Result<impl Stream<Item = UploadProgress>>` —
the stream shape reads well in async Rust and lets the caller drop it to
cancel. State machine, transcribed from the Go code:

1. Compute MD5 (streamed from disk, don't slurp).
2. Send `FileStartAsk` with size + MD5 + type. Device replies with resume
   offset.
3. Loop: read chunk from file (max `upload_chunk_size` bytes), send
   `FileContentAsk`, await `FileContentReply`, emit progress.
4. Send `FileEndAsk`, await final ACK.

Chunk size and retry policy live on `DeviceConfig`.

---

## 9. Configuration

Plain struct with `Default`. No functional-options pattern.

```rust
#[derive(Debug, Clone)]
pub struct DeviceConfig {
    pub timeout: Duration,
    pub heartbeat: Duration,
    pub upload_chunk_size: usize,
    pub sdk_fragment_size: usize,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            heartbeat: Duration::from_secs(30),
            upload_chunk_size: 8000,
            sdk_fragment_size: 8000,
        }
    }
}
```

Logging: `tracing`. No custom-logger option — users configure a subscriber.

---

## 10. Testing strategy

Three tiers, all runnable with `cargo test` and no hardware.

**Tier 1 — codec unit tests (`huidu-proto`).** Golden byte sequences ported
from the Go project's `diagnose_test.go` and `hd2020_gen6_test.go`. Every wire
message type has round-trip tests: `encode → decode → assert_eq`, and
`decode(golden) → matches expected`.

**Tier 2 — mock device (`huidu` dev-dependency).** A `MockDevice` that binds
`127.0.0.1:0`, accepts one connection, and replays a scripted sequence of
expected requests / canned responses. Reproduces the 3-phase handshake and
enough command flows to test `send_screen`, `set_brightness`, `upload_file`,
etc.

**Tier 3 — hardware tests behind a feature flag.** `cargo test --features
hardware-tests` runs against a real device at a `HUIDU_TEST_ADDR` env var.
Skipped in CI.

Golden fixtures live in `tests/fixtures/`, one `.bin` per captured message,
with a `.txt` sidecar documenting provenance ("captured from HD-WF2 firmware
v1.2.3, 2026-07-15").

---

## 11. Open questions & tradeoffs

1. **Cancellation model** (see §4.4). Simple poisoning vs. dedicated reader
   task with GUID correlation. Simple is fine for v0; correlation is a v1
   concern if users report footguns.
2. **`bytes::Bytes` vs `Vec<u8>` in the public API.** `Bytes` avoids copies
   when receiving; `Vec<u8>` is more familiar. Proposed: `Bytes` internally,
   `&[u8]` in public method signatures, `Vec<u8>` only where the caller must
   own.
3. **`Screen` — one type or `SdkScreen` + `Hd2020Screen`?** Some fields
   (multi-program, transitions) don't apply to HD2020. Runtime rejection is
   honest; separate types are safer but double the API surface. Proposed:
   one type + validation at `send_screen` time.
4. **Font asset licensing** for HD2020. Plan9 bitmap font is BSD-licensed and
   what `basicfont` uses — safe to include. Confirm before shipping.
5. **Which `tokio` version.** Pin to `1.x` (latest stable). No reason to
   support `0.x`.
6. **MSRV.** Propose Rust 1.75 (stable async traits) or later; revisit if we
   pull in a crate that pushes the floor up.

---

## 12. Subsystem breakdown for implementation plans

Full parity is too big for one plan. Break into these, roughly in dependency
order. Each becomes an input to the `superpowers:writing-plans` skill
separately:

1. **`huidu-proto` core** — raw frame codec, SDK fragment reassembly, error
   types, first golden fixtures. No I/O.
2. **`huidu-proto` SDK messages** — all 24 XML request/response types,
   round-trip tests.
3. **`huidu-proto` HD2020** — framing, bitmap encoding, font bundling, golden
   fixtures ported from Go tests.
4. **`huidu` transport & handshake** — `Device::connect`, heartbeat, close.
5. **`huidu` SDK command surface** — `get_device_info`, `set_brightness`,
   network config, time sync, boot logo, TCP server config, file listing.
   Mechanical command wrappers, one plan.
6. **`huidu` screen builder & send** — `Screen`/`Program`/`Area`/`Item` types,
   `send_screen`.
7. **`huidu` file upload** — MD5 streaming, chunked upload state machine,
   progress stream.
8. **`huidu` HD2020 dispatch** — protocol probing, HD2020-specific command
   routing.

Plans 5–8 can proceed in parallel once 1–4 land.
