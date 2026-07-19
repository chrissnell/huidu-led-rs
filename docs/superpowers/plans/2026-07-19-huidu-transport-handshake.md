# Plan: `huidu` transport & handshake

**Subsystem 4 of 8** (GRA-395). Reference: `DESIGN.md` §4.1–§4.4, §7, §9.

**Goal:** Stand up the `huidu` async client crate with the connection type, the
3-phase handshake, a background heartbeat, graceful `close`, the poison-on-cancel
cancellation model, `DeviceConfig`, and a reusable Tier-2 `MockDevice` test
harness. Builds on `huidu-proto` (subsystems 1 & 2) for framing and XML.

## Layout

```
crates/huidu/
├── Cargo.toml
├── src/
│   ├── lib.rs        # re-exports; crate docs
│   ├── error.rs      # Error, ProtocolKind, Result alias (DESIGN §7)
│   ├── config.rs     # DeviceConfig + Default (DESIGN §9)
│   ├── transport.rs  # frame/SDK roundtrip over Framed<TcpStream, HuiduCodec>
│   └── device.rs     # Device, DeviceInner, connect, close, heartbeat
└── tests/
    ├── common/mod.rs # MockDevice scripted harness (shared by later subsystems)
    └── handshake.rs  # Tier-2 handshake integration tests
```

`MockDevice` lives under `tests/common/` — the standard Rust shared-integration-
test pattern — so subsystems 5–7 can `mod common;` and reuse it, matching
`DESIGN.md §10`'s "mock device (dev-dependency)".

## Transport core (`transport.rs`)

I/O-free-adjacent helpers operating on `&mut Framed<TcpStream, HuiduCodec>`, so
they serve both the handshake (direct, pre-`Arc`) and `DeviceInner` (post-lock):

- `raw_roundtrip(framed, request, expect, timeout)` — send one raw frame, await
  one reply frame, assert its `CmdCode`. Used by version handshake + heartbeat.
- `sdk_roundtrip(framed, xml, config)` — fragment `xml` into `SdkFragment`s
  sized by `sdk_fragment_size`, send each as an `SdkCmd` frame, then read
  `SdkReply` frames and feed an `SdkReassembler` until the XML is complete.
  Timed by `config.timeout`.

## Cancellation (DESIGN §4.4)

A `PoisonGuard` drop-guard arms before the first wire write and disarms only on
full success. Any early return — a dropped future (cancel) or a mid-roundtrip
error — leaves the guard armed, which stores `poisoned = true`. Every subsequent
command checks the flag under the lock and returns `Error::Poisoned`.

## Device (`device.rs`)

```rust
pub struct Device { inner: Arc<DeviceInner> }

struct DeviceInner {
    frames: tokio::sync::Mutex<Framed<TcpStream, HuiduCodec>>,
    guid: String,             // session guid, set at connect (device-controlled string)
    info: DeviceInfo,         // cached at connect
    config: DeviceConfig,
    protocol: ProtocolKind,   // Sdk2 in v0; HD2020 probing lands in subsystem 8
    heartbeat: Mutex<Option<JoinHandle<()>>>,
    poisoned: AtomicBool,
}
```

`guid`/`info` are plain fields (not `OnceCell`): the handshake runs on the bare
`Framed` before it moves behind the mutex/`Arc`, so they are known at construction.
The session guid is kept as the raw device string — it is echoed back verbatim on
every command, so reformatting through `Uuid` would risk breaking the session.

### `connect(addr, config)` — 3 phases (DESIGN §4.2)

1. `VersionAsk` → await `VersionReply` (payload opaque in v0 — see open question).
2. `GetIFVersion` SDK request with `##GUID` placeholder → reply's `<sdk guid>` is
   the session guid.
3. `GetDeviceInfo` SDK request with the session guid → decode + cache `DeviceInfo`.

Each phase's failure is wrapped as `Error::Handshake { phase, source }` and the
socket is dropped. No auto-reconnect. On success, spawn the heartbeat task
(`Weak<DeviceInner>`, `config.heartbeat` interval) and store its `JoinHandle`.

### `close(self)` — abort the heartbeat task, drop the connection.

### Accessors — `guid()`, `info()`, `protocol()`.

## Testing (Tier 2)

`MockDevice::spawn(script)` binds `127.0.0.1:0`, accepts one connection, and
replays a scripted sequence, auto-answering heartbeats. A `Script::handshake(
guid, info)` convenience scripts the 3 phases. Tests:

- handshake succeeds; `guid()`/`info()` match the mock's script.
- a phase-2/3 device error surfaces as `Error::Handshake { phase, .. }`.
- `close` is clean and idempotent-safe.
- heartbeat ping/reply round-trips (short interval, mock answers).

## Open question

`VersionAsk`/`VersionReply` payload contents are unspecified in `DESIGN.md §3`.
v0 sends an empty `VersionAsk` and treats the reply payload as opaque, asserting
only the `VersionReply` command code. Confirm against a real capture or the Go
reference before locking a fixture.
