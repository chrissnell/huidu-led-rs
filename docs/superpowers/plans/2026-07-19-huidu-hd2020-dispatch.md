# Plan: `huidu` HD2020 dispatch

**Subsystem 8 of 8** (GRA-399). Reference: `DESIGN.md` §6, §7, §11 item 3, §12
item 8.

**Goal:** Detect the wire protocol at connect time from the version-probe reply,
branch the handshake accordingly, gate every command behind the protocol it
belongs to (`Error::UnsupportedForProtocol` when misused), and wire an HD2020
realtime-text send path to the subsystem-3 bitmap encoder. Resolve the open
`Screen`-type question (`DESIGN.md §11` item 3).

## Protocol probing (`DESIGN.md §6`)

The version handshake (`VersionAsk` → `VersionReply`) already runs first on the
bare socket. It becomes the **probe**: the reply payload tells us which protocol
the device speaks. New `probe.rs`:

- `classify(payload: &[u8]) -> ProtocolKind` — design-derived: an HD2020 Gen6
  controller answers with a payload led by the HD2020 start byte
  (`hd2020::START` = `0xA5`); anything else (including the empty SDK2 reply) is
  `Sdk2`.

Like the HD2020 framing constants, the exact probe-reply shape has no capture or
Go reference in this workspace; the classifier is a self-consistent placeholder
isolated in one small, unit-tested function so only its body changes when a real
capture lands.

## Connect branch (`device.rs`)

`Device::connect` after phase 1:

1. `raw_roundtrip(VersionAsk → VersionReply)` — now keeps the reply.
2. `classify(reply.payload)`:
   - `Sdk2` → run phases 2–3 (`GetIFVersion`, `GetDeviceInfo`) exactly as today;
     build `DeviceInner { protocol: Sdk2, guid, info }`.
   - `Hd2020` → **no SDK XML handshake** (HD2020 doesn't speak SDK 2.0 XML).
     Build `DeviceInner { protocol: Hd2020, guid: "", info: DeviceInfo::default() }`.
     `guid()`/`info()` are documented as unpopulated on HD2020 in v0.

Heartbeat still runs for both; `Heartbeat`/`HeartbeatReply` share the raw frame
envelope.

## Dispatch gating (`DESIGN.md §6`)

`DeviceInner::require(kind) -> Result<()>` returns
`Error::UnsupportedForProtocol(self.protocol)` when `self.protocol != kind`.

- `send_sdk` (the SDK command surface's foundation) calls `require(Sdk2)` first —
  so every SDK command returns `UnsupportedForProtocol` on an HD2020 device.
- `send_realtime_text` calls `require(Hd2020)` — so the HD2020 path returns
  `UnsupportedForProtocol` on an SDK2 device. Honest both ways, unlike the Go
  reference.

## HD2020 realtime-text send path

`Device::send_realtime_text(text, width, height, &TextLayout) -> Result<()>`:

- `require(Hd2020)`.
- Render + frame via subsystem 3: `hd2020::realtime_text_frame(...)` →
  `Bytes` of a native `0xA5`-framed HD2020 packet.
- Write the raw frame under the connection mutex. HD2020 frames do **not** use
  `HuiduCodec`, so `transport::hd2020_send` writes straight to the underlying
  `TcpStream` (`Framed::get_mut`) and flushes. v0 realtime pushes are
  fire-and-forget (no ack shape is specified), guarded by `PoisonGuard` so a
  cancelled write still poisons the connection.

## `Screen`-type decision (`DESIGN.md §11` item 3) — RESOLVED

**Decision: one `Screen` type + validation at `send_screen` time** (the design's
proposed option), not an `SdkScreen`/`Hd2020Screen` split.

Rationale: HD2020 realtime text is a fundamentally different, bitmap-oriented
path with its own entry point (`send_realtime_text`) — it does not consume the
SDK `Screen` tree at all. A split would double the builder API surface to model a
divergence that the protocol gate already expresses honestly at runtime. The SDK
`Screen` (subsystem 6) stays a single type; `send_screen` lives on the SDK path
and returns `UnsupportedForProtocol` on an HD2020 connection via the same
`require(Sdk2)` gate. This note records the resolution for subsystem 6.

## Tests (Tier-2, `tests/hd2020.rs`)

Extend `MockDevice` with a `version_payload(Bytes)` override so a mock can answer
the probe as either protocol.

- **SDK2 probe** → `connect` yields `ProtocolKind::Sdk2`, full handshake, and an
  SDK command works while `send_realtime_text` returns `UnsupportedForProtocol`.
- **HD2020 probe** → `connect` yields `ProtocolKind::Hd2020` without SDK phases;
  `send_realtime_text` writes a well-formed `0xA5` frame the mock decodes back to
  the expected `RealtimeText` bitmap; an SDK command returns
  `UnsupportedForProtocol`.
- `classify` unit tests for both payload shapes and the empty reply.
