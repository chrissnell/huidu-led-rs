# `huidu-proto` Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the I/O-free foundation of the `huidu-proto` crate — the raw TCP frame codec, the SDK fragmentation envelope, the error type, and the first golden fixtures — so every later subsystem has a tested `bytes ↔ frame` layer to build on.

**Architecture:** A single library crate `huidu-proto` inside a Cargo workspace. It exposes a `tokio_util::codec` `Decoder`/`Encoder` pair (`HuiduCodec`) that turns a byte stream into owned `OwnedFrame` values and back, plus an `SdkFragment`/`SdkReassembler` pair that handles the `total_len`/`offset` XML fragmentation envelope carried inside SDK frames. There is no networking, no `tokio` runtime, and no XML parsing in this plan — those are later subsystems. Everything is tested against in-memory `BytesMut` buffers and committed golden `.bin` fixtures.

**Tech Stack:** Rust (edition 2021), `bytes`, `tokio-util` (codec feature), `thiserror`.

**Scope note:** This is subsystem **1 of 8** from `DESIGN.md §12`. It deliberately excludes the 24 XML message types (subsystem 2), HD2020 framing (subsystem 3), and everything in the `huidu` crate (subsystems 4–8). The `ProtoError` enum and `lib.rs` re-exports created here are grown by later plans; this plan adds only the variants and modules it actually uses. `DESIGN.md §6` reorganizes protocol-specific code under `sdk2/` and `hd2020/`, but the raw frame codec is common to both protocols, so it lives at the crate `src/` root exactly as shown in `DESIGN.md §3`.

**Wire-format decision to lock in (and verify):** `DESIGN.md §3.1` states every packet is `<len:u16le><cmd:u16le><payload>` but does not spell out what `len` counts. This plan fixes the convention as: **`len` = the number of bytes that follow the length field = 2 (cmd) + payload length**, little-endian. All fixtures and tests below assume this. During implementation, confirm against a real device capture or the Go reference (`../huidu-led/`); if the real wire uses a different span (e.g. total-including-len), only the two `LEN_BYTES`/`CMD_BYTES` arithmetic sites in `codec.rs` and the fixture bytes change — the rest of the plan is unaffected.

---

### Task 0: Workspace and crate scaffold

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/huidu-proto/Cargo.toml`
- Create: `crates/huidu-proto/src/lib.rs`

- [ ] **Step 1: Create the workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/huidu-proto"]
```

- [ ] **Step 2: Create the crate manifest**

Create `crates/huidu-proto/Cargo.toml`:

```toml
[package]
name = "huidu-proto"
version = "0.0.0"
edition = "2021"
license = "MIT"
description = "Wire framing and codec for the Huidu SDK 2.0 / HD2020 LED controller protocol. No I/O."

[dependencies]
bytes = "1"
thiserror = "1"
tokio-util = { version = "0.7", features = ["codec"] }
```

- [ ] **Step 3: Create an empty crate root**

Create `crates/huidu-proto/src/lib.rs`:

```rust
//! I/O-free wire protocol layer for Huidu LED controllers.
//!
//! Turns raw TCP bytes into typed frames and back, and reassembles fragmented
//! SDK XML payloads. No networking lives here — see the `huidu` crate for that.
```

- [ ] **Step 4: Verify the workspace builds**

Run: `cargo build`
Expected: PASS — `Compiling huidu-proto v0.0.0`, finishes with no errors or warnings.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/huidu-proto/Cargo.toml crates/huidu-proto/src/lib.rs
git commit -m "chore: scaffold huidu-proto crate and workspace"
```

---

### Task 1: Error type

**Files:**
- Create: `crates/huidu-proto/src/error.rs`
- Modify: `crates/huidu-proto/src/lib.rs`

Only the variants this plan uses are added now. `DESIGN.md §7` lists more (`Xml`, `SdkError`, `Hd2020`); those are added by subsystems 2 and 3 when their code first needs them, to avoid referencing types (`SdkResult`, `quick_xml::Error`) that do not exist yet.

- [ ] **Step 1: Write the error module**

Create `crates/huidu-proto/src/error.rs`:

```rust
//! The single error type returned by everything in this crate.

/// Errors produced while framing, decoding, or reassembling wire messages.
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    /// A frame or payload was shorter than the minimum its shape requires.
    #[error("frame too short: expected {expected} bytes, got {got}")]
    ShortFrame { expected: usize, got: usize },

    /// The command field held a value we do not recognize.
    #[error("unknown command code: 0x{0:04x}")]
    UnknownCmd(u16),

    /// A frame's payload exceeds what the u16 length prefix can describe.
    #[error("frame payload too large: {0} bytes exceeds u16 length field")]
    FrameTooLarge(usize),

    /// An SDK fragment did not arrive at the next expected byte offset.
    #[error("sdk fragment offset {got} does not match expected {expected}")]
    FragmentGap { expected: u32, got: u32 },

    /// An SDK fragment would push the reassembled buffer past its declared length.
    #[error("sdk fragment overflows declared total length {total}: offset {offset} + {chunk} bytes")]
    FragmentOverflow { total: u32, offset: u32, chunk: usize },
}
```

- [ ] **Step 2: Register the module and re-export**

In `crates/huidu-proto/src/lib.rs`, add below the doc comment:

```rust
pub mod error;

pub use error::ProtoError;
```

- [ ] **Step 3: Verify it builds**

Run: `cargo build`
Expected: PASS, no warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/huidu-proto/src/error.rs crates/huidu-proto/src/lib.rs
git commit -m "feat(proto): add ProtoError type"
```

---

### Task 2: Frame types

**Files:**
- Create: `crates/huidu-proto/src/frame.rs`
- Modify: `crates/huidu-proto/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/huidu-proto/src/frame.rs`:

```rust
//! Command codes and the owned frame value the codec hands out.

use crate::error::ProtoError;
use bytes::Bytes;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_code_roundtrips_through_u16() {
        let code = CmdCode::from_u16(0x2005).unwrap();
        assert_eq!(code, CmdCode::Heartbeat);
        assert_eq!(code.as_u16(), 0x2005);
    }

    #[test]
    fn unknown_code_errors() {
        let err = CmdCode::from_u16(0x9999).unwrap_err();
        assert!(matches!(err, ProtoError::UnknownCmd(0x9999)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p huidu-proto frame::`
Expected: FAIL — `cannot find type CmdCode in this scope`.

- [ ] **Step 3: Write the minimal implementation**

At the top of `crates/huidu-proto/src/frame.rs`, above the `#[cfg(test)]` block, add:

```rust
/// Every SDK 2.0 / file-transfer command code. HD2020 codes are added by the
/// HD2020 subsystem plan.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdCode {
    VersionAsk = 0x2001,
    VersionReply = 0x2002,
    SdkCmd = 0x2003,
    SdkReply = 0x2004,
    Heartbeat = 0x2005,
    HeartbeatReply = 0x2006,
    FileStartAsk = 0x8001,
    FileStartReply = 0x8002,
    FileContentAsk = 0x8003,
    FileContentReply = 0x8004,
    FileEndAsk = 0x8005,
    FileEndReply = 0x8006,
}

impl CmdCode {
    /// Parse a raw command word, rejecting unknown codes.
    pub fn from_u16(v: u16) -> Result<Self, ProtoError> {
        Ok(match v {
            0x2001 => Self::VersionAsk,
            0x2002 => Self::VersionReply,
            0x2003 => Self::SdkCmd,
            0x2004 => Self::SdkReply,
            0x2005 => Self::Heartbeat,
            0x2006 => Self::HeartbeatReply,
            0x8001 => Self::FileStartAsk,
            0x8002 => Self::FileStartReply,
            0x8003 => Self::FileContentAsk,
            0x8004 => Self::FileContentReply,
            0x8005 => Self::FileEndAsk,
            0x8006 => Self::FileEndReply,
            other => return Err(ProtoError::UnknownCmd(other)),
        })
    }

    /// The raw command word for this code.
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

/// A decoded frame that owns its payload — no lifetime tied to the read buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedFrame {
    pub cmd: CmdCode,
    pub payload: Bytes,
}

impl OwnedFrame {
    /// Build a frame from a command code and any byte container.
    pub fn new(cmd: CmdCode, payload: impl Into<Bytes>) -> Self {
        Self {
            cmd,
            payload: payload.into(),
        }
    }
}
```

- [ ] **Step 4: Register the module and re-export**

In `crates/huidu-proto/src/lib.rs`, add:

```rust
pub mod frame;

pub use frame::{CmdCode, OwnedFrame};
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p huidu-proto frame::`
Expected: PASS — `known_code_roundtrips_through_u16 ... ok`, `unknown_code_errors ... ok`.

- [ ] **Step 6: Commit**

```bash
git add crates/huidu-proto/src/frame.rs crates/huidu-proto/src/lib.rs
git commit -m "feat(proto): add CmdCode and OwnedFrame"
```

---

### Task 3: Frame codec (Decoder + Encoder)

**Files:**
- Create: `crates/huidu-proto/src/codec.rs`
- Modify: `crates/huidu-proto/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/huidu-proto/src/codec.rs`:

```rust
//! `tokio_util::codec` Decoder/Encoder for the raw `<len><cmd><payload>` frame.

use crate::error::ProtoError;
use crate::frame::{CmdCode, OwnedFrame};
use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

/// Bytes in the little-endian length prefix.
const LEN_BYTES: usize = 2;
/// Bytes in the little-endian command field.
const CMD_BYTES: usize = 2;

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn encodes_heartbeat() {
        let mut codec = HuiduCodec;
        let mut out = BytesMut::new();
        codec
            .encode(OwnedFrame::new(CmdCode::Heartbeat, Bytes::new()), &mut out)
            .unwrap();
        // len = 2 (cmd only), cmd = 0x2005, no payload.
        assert_eq!(&out[..], &[0x02, 0x00, 0x05, 0x20]);
    }

    #[test]
    fn encode_decode_roundtrips_with_payload() {
        let mut codec = HuiduCodec;
        let frame = OwnedFrame::new(CmdCode::SdkCmd, Bytes::from_static(b"abc"));
        let mut buf = BytesMut::new();
        codec.encode(frame.clone(), &mut buf).unwrap();
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded, frame);
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_waits_for_full_frame() {
        let mut codec = HuiduCodec;
        // Full frame is 0x02 0x00 0x05 0x20; feed only the length prefix first.
        let mut buf = BytesMut::from(&[0x02u8, 0x00][..]);
        assert!(codec.decode(&mut buf).unwrap().is_none());
        buf.extend_from_slice(&[0x05, 0x20]);
        let frame = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame.cmd, CmdCode::Heartbeat);
    }

    #[test]
    fn decode_rejects_unknown_command() {
        let mut codec = HuiduCodec;
        // len = 2, cmd = 0x9999 (unknown).
        let mut buf = BytesMut::from(&[0x02u8, 0x00, 0x99, 0x99][..]);
        let err = codec.decode(&mut buf).unwrap_err();
        assert!(matches!(err, ProtoError::UnknownCmd(0x9999)));
    }

    #[test]
    fn decode_handles_two_frames_in_one_buffer() {
        let mut codec = HuiduCodec;
        let mut buf = BytesMut::new();
        codec
            .encode(OwnedFrame::new(CmdCode::Heartbeat, Bytes::new()), &mut buf)
            .unwrap();
        codec
            .encode(OwnedFrame::new(CmdCode::HeartbeatReply, Bytes::new()), &mut buf)
            .unwrap();
        assert_eq!(codec.decode(&mut buf).unwrap().unwrap().cmd, CmdCode::Heartbeat);
        assert_eq!(
            codec.decode(&mut buf).unwrap().unwrap().cmd,
            CmdCode::HeartbeatReply
        );
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p huidu-proto codec::`
Expected: FAIL — `cannot find type HuiduCodec in this scope`.

- [ ] **Step 3: Write the minimal implementation**

In `crates/huidu-proto/src/codec.rs`, above the `#[cfg(test)]` block, add:

```rust
/// Stateless codec for the raw framing layer.
pub struct HuiduCodec;

impl Decoder for HuiduCodec {
    type Item = OwnedFrame;
    type Error = ProtoError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < LEN_BYTES {
            return Ok(None);
        }
        // `len` counts the cmd field plus the payload — see the plan's wire-format note.
        let len = u16::from_le_bytes([src[0], src[1]]) as usize;
        if len < CMD_BYTES {
            return Err(ProtoError::ShortFrame {
                expected: CMD_BYTES,
                got: len,
            });
        }
        let total = LEN_BYTES + len;
        if src.len() < total {
            src.reserve(total - src.len());
            return Ok(None);
        }
        src.advance(LEN_BYTES);
        let cmd = CmdCode::from_u16(u16::from_le_bytes([src[0], src[1]]))?;
        src.advance(CMD_BYTES);
        let payload = src.split_to(len - CMD_BYTES).freeze();
        Ok(Some(OwnedFrame { cmd, payload }))
    }
}

impl Encoder<OwnedFrame> for HuiduCodec {
    type Error = ProtoError;

    fn encode(&mut self, frame: OwnedFrame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let len = CMD_BYTES + frame.payload.len();
        let len_u16 = u16::try_from(len).map_err(|_| ProtoError::FrameTooLarge(len))?;
        dst.reserve(LEN_BYTES + len);
        dst.put_u16_le(len_u16);
        dst.put_u16_le(frame.cmd.as_u16());
        dst.put_slice(&frame.payload);
        Ok(())
    }
}
```

- [ ] **Step 4: Register the module and re-export**

In `crates/huidu-proto/src/lib.rs`, add:

```rust
pub mod codec;

pub use codec::HuiduCodec;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p huidu-proto codec::`
Expected: PASS — all five codec tests `... ok`.

- [ ] **Step 6: Commit**

```bash
git add crates/huidu-proto/src/codec.rs crates/huidu-proto/src/lib.rs
git commit -m "feat(proto): add HuiduCodec frame decoder/encoder"
```

---

### Task 4: SDK fragment parse/encode

**Files:**
- Create: `crates/huidu-proto/src/sdk_frame.rs`
- Modify: `crates/huidu-proto/src/lib.rs`

An SDK frame (`CmdCode::SdkCmd` / `SdkReply`) carries, inside its payload, an 8-byte envelope — `total_len:u32le` then `offset:u32le` — followed by an XML fragment (`DESIGN.md §3.3`). This task handles a single fragment's bytes; reassembly across fragments is Task 5.

- [ ] **Step 1: Write the failing tests**

Create `crates/huidu-proto/src/sdk_frame.rs`:

```rust
//! The SDK XML fragmentation envelope carried inside 0x2003 / 0x2004 frames.

use crate::error::ProtoError;
use bytes::{BufMut, Bytes, BytesMut};

/// Bytes in the fragment header: total_len (u32) + offset (u32).
const HEADER_BYTES: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_header_and_chunk() {
        // total_len = 5, offset = 0, chunk = "hello".
        let payload = [
            0x05, 0x00, 0x00, 0x00, // total_len
            0x00, 0x00, 0x00, 0x00, // offset
            b'h', b'e', b'l', b'l', b'o',
        ];
        let frag = SdkFragment::parse(&payload).unwrap();
        assert_eq!(frag.total_len, 5);
        assert_eq!(frag.offset, 0);
        assert_eq!(&frag.xml_chunk[..], b"hello");
    }

    #[test]
    fn parse_rejects_short_payload() {
        let err = SdkFragment::parse(&[0x00, 0x01, 0x02]).unwrap_err();
        assert!(matches!(err, ProtoError::ShortFrame { expected: 8, got: 3 }));
    }

    #[test]
    fn encode_parse_roundtrips() {
        let frag = SdkFragment {
            total_len: 12,
            offset: 4,
            xml_chunk: Bytes::from_static(b"world!"),
        };
        let bytes = frag.encode();
        assert_eq!(SdkFragment::parse(&bytes).unwrap(), frag);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p huidu-proto sdk_frame::`
Expected: FAIL — `cannot find type SdkFragment in this scope`.

- [ ] **Step 3: Write the minimal implementation**

In `crates/huidu-proto/src/sdk_frame.rs`, above the `#[cfg(test)]` block, add:

```rust
/// One SDK XML fragment: the envelope header plus this fragment's XML bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkFragment {
    pub total_len: u32,
    pub offset: u32,
    pub xml_chunk: Bytes,
}

impl SdkFragment {
    /// Parse the payload of an SDK frame into a fragment.
    pub fn parse(payload: &[u8]) -> Result<Self, ProtoError> {
        if payload.len() < HEADER_BYTES {
            return Err(ProtoError::ShortFrame {
                expected: HEADER_BYTES,
                got: payload.len(),
            });
        }
        let total_len = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let offset = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        let xml_chunk = Bytes::copy_from_slice(&payload[HEADER_BYTES..]);
        Ok(Self {
            total_len,
            offset,
            xml_chunk,
        })
    }

    /// Serialize this fragment into an SDK frame payload.
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(HEADER_BYTES + self.xml_chunk.len());
        buf.put_u32_le(self.total_len);
        buf.put_u32_le(self.offset);
        buf.put_slice(&self.xml_chunk);
        buf.freeze()
    }
}
```

- [ ] **Step 4: Register the module and re-export**

In `crates/huidu-proto/src/lib.rs`, add:

```rust
pub mod sdk_frame;

pub use sdk_frame::SdkFragment;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p huidu-proto sdk_frame::`
Expected: PASS — all three fragment tests `... ok`.

- [ ] **Step 6: Commit**

```bash
git add crates/huidu-proto/src/sdk_frame.rs crates/huidu-proto/src/lib.rs
git commit -m "feat(proto): add SdkFragment envelope parse/encode"
```

---

### Task 5: SDK reassembler

**Files:**
- Modify: `crates/huidu-proto/src/sdk_frame.rs`
- Modify: `crates/huidu-proto/src/lib.rs`

The transport is strict request/response serialized through one mutex (`DESIGN.md §4.1`), so at most one SDK message is being reassembled at a time. A single accumulator that checks each fragment lands at the next expected offset and returns the complete XML when the buffer reaches `total_len` is sufficient. The `(guid, method)`-keyed variant from `DESIGN.md §3.3` is only needed once a multiplexing reader task exists (`DESIGN.md §4.4`, a v1 concern) — do not build it now.

- [ ] **Step 1: Write the failing tests**

In `crates/huidu-proto/src/sdk_frame.rs`, add these tests inside the existing `#[cfg(test)] mod tests { ... }` block:

```rust
    #[test]
    fn reassembles_single_fragment() {
        let mut r = SdkReassembler::new();
        let frag = SdkFragment {
            total_len: 5,
            offset: 0,
            xml_chunk: Bytes::from_static(b"hello"),
        };
        assert_eq!(r.push(frag).unwrap().as_deref(), Some(&b"hello"[..]));
    }

    #[test]
    fn reassembles_two_fragments() {
        let mut r = SdkReassembler::new();
        let first = SdkFragment {
            total_len: 10,
            offset: 0,
            xml_chunk: Bytes::from_static(b"hello"),
        };
        let second = SdkFragment {
            total_len: 10,
            offset: 5,
            xml_chunk: Bytes::from_static(b"world"),
        };
        assert!(r.push(first).unwrap().is_none());
        assert_eq!(r.push(second).unwrap().as_deref(), Some(&b"helloworld"[..]));
    }

    #[test]
    fn rejects_out_of_order_fragment() {
        let mut r = SdkReassembler::new();
        let first = SdkFragment {
            total_len: 10,
            offset: 0,
            xml_chunk: Bytes::from_static(b"hello"),
        };
        r.push(first).unwrap();
        let bad = SdkFragment {
            total_len: 10,
            offset: 7, // expected 5
            xml_chunk: Bytes::from_static(b"xyz"),
        };
        let err = r.push(bad).unwrap_err();
        assert!(matches!(err, ProtoError::FragmentGap { expected: 5, got: 7 }));
    }

    #[test]
    fn rejects_overflowing_fragment() {
        let mut r = SdkReassembler::new();
        let frag = SdkFragment {
            total_len: 3,
            offset: 0,
            xml_chunk: Bytes::from_static(b"toolong"),
        };
        let err = r.push(frag).unwrap_err();
        assert!(matches!(
            err,
            ProtoError::FragmentOverflow {
                total: 3,
                offset: 0,
                chunk: 7
            }
        ));
    }

    #[test]
    fn resets_after_completion() {
        let mut r = SdkReassembler::new();
        let one = SdkFragment {
            total_len: 3,
            offset: 0,
            xml_chunk: Bytes::from_static(b"abc"),
        };
        assert_eq!(r.push(one).unwrap().as_deref(), Some(&b"abc"[..]));
        // A fresh message reuses the same reassembler starting at offset 0.
        let two = SdkFragment {
            total_len: 2,
            offset: 0,
            xml_chunk: Bytes::from_static(b"de"),
        };
        assert_eq!(r.push(two).unwrap().as_deref(), Some(&b"de"[..]));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p huidu-proto sdk_frame::`
Expected: FAIL — `cannot find type SdkReassembler in this scope`.

- [ ] **Step 3: Write the minimal implementation**

In `crates/huidu-proto/src/sdk_frame.rs`, add after the `impl SdkFragment` block (before the `#[cfg(test)]` block):

```rust
/// Accumulates SDK fragments into a complete XML payload.
///
/// Assumes one in-flight message at a time, which holds because the transport
/// serializes every request/response through a single mutex (see `DESIGN.md §4.1`).
#[derive(Debug, Default)]
pub struct SdkReassembler {
    total_len: u32,
    buf: BytesMut,
}

impl SdkReassembler {
    /// A fresh reassembler with no message in progress.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one fragment. Returns `Some(xml)` once the whole message has arrived,
    /// then resets for the next message.
    pub fn push(&mut self, frag: SdkFragment) -> Result<Option<Bytes>, ProtoError> {
        if self.buf.is_empty() {
            self.total_len = frag.total_len;
        }
        let expected = self.buf.len() as u32;
        if frag.offset != expected {
            return Err(ProtoError::FragmentGap {
                expected,
                got: frag.offset,
            });
        }
        let end = frag.offset as usize + frag.xml_chunk.len();
        if end > frag.total_len as usize {
            return Err(ProtoError::FragmentOverflow {
                total: frag.total_len,
                offset: frag.offset,
                chunk: frag.xml_chunk.len(),
            });
        }
        self.buf.put_slice(&frag.xml_chunk);
        if self.buf.len() as u32 == self.total_len {
            let complete = std::mem::take(&mut self.buf).freeze();
            self.total_len = 0;
            return Ok(Some(complete));
        }
        Ok(None)
    }
}
```

- [ ] **Step 4: Register the re-export**

In `crates/huidu-proto/src/lib.rs`, change the `sdk_frame` re-export line to include the reassembler:

```rust
pub use sdk_frame::{SdkFragment, SdkReassembler};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p huidu-proto sdk_frame::`
Expected: PASS — all fragment and reassembler tests `... ok`.

- [ ] **Step 6: Commit**

```bash
git add crates/huidu-proto/src/sdk_frame.rs crates/huidu-proto/src/lib.rs
git commit -m "feat(proto): add SdkReassembler"
```

---

### Task 6: Golden fixtures and integration test

**Files:**
- Create: `crates/huidu-proto/tests/fixtures/heartbeat.bin`
- Create: `crates/huidu-proto/tests/fixtures/heartbeat.txt`
- Create: `crates/huidu-proto/tests/fixtures/sdk_hello.bin`
- Create: `crates/huidu-proto/tests/fixtures/sdk_hello.txt`
- Create: `crates/huidu-proto/tests/golden.rs`

Golden `.bin` files live with the crate that consumes them (`CARGO_MANIFEST_DIR`-relative), each with a `.txt` sidecar documenting provenance, per `DESIGN.md §10`. The repo-root `tests/fixtures/` in `DESIGN.md §2` is aspirational for cross-crate captures; crate integration tests read from the crate's own `tests/fixtures/`.

- [ ] **Step 1: Write the heartbeat fixture and its sidecar**

```bash
mkdir -p crates/huidu-proto/tests/fixtures
printf '\x02\x00\x05\x20' > crates/huidu-proto/tests/fixtures/heartbeat.bin
```

Create `crates/huidu-proto/tests/fixtures/heartbeat.txt`:

```
Heartbeat frame (CmdCode::Heartbeat, 0x2005), no payload.
Bytes: 02 00 05 20
  02 00  len = 2 (cmd field only)
  05 20  cmd = 0x2005 little-endian
Provenance: hand-constructed from DESIGN.md §3.1 command table, 2026-07-18.
Replace with a real device capture when hardware is available.
```

- [ ] **Step 2: Write the SDK-hello fixture and its sidecar**

```bash
printf '\x0f\x00\x03\x20\x05\x00\x00\x00\x00\x00\x00\x00hello' > crates/huidu-proto/tests/fixtures/sdk_hello.bin
```

Create `crates/huidu-proto/tests/fixtures/sdk_hello.txt`:

```
SDK command frame (CmdCode::SdkCmd, 0x2003) carrying one fragment: total_len=5,
offset=0, xml_chunk="hello".
Bytes: 0f 00 03 20 05 00 00 00 00 00 00 00 68 65 6c 6c 6f
  0f 00                    len = 15 (cmd 2 + payload 13)
  03 20                    cmd = 0x2003 little-endian
  05 00 00 00              fragment total_len = 5
  00 00 00 00              fragment offset = 0
  68 65 6c 6c 6f           xml_chunk = "hello"
Provenance: hand-constructed from DESIGN.md §3.1 / §3.3, 2026-07-18.
Replace with a real device capture when hardware is available.
```

- [ ] **Step 3: Verify the fixture bytes are exactly right**

Run: `xxd crates/huidu-proto/tests/fixtures/heartbeat.bin && echo '---' && xxd crates/huidu-proto/tests/fixtures/sdk_hello.bin`
Expected:
```
00000000: 0200 0520                                ...
---
00000000: 0f00 0320 0500 0000 0000 0000 6865 6c6c  ... ........hell
00000010: 6f                                       o
```
If `heartbeat.bin` is not exactly 4 bytes or `sdk_hello.bin` not exactly 17 bytes, the `printf` escaping went wrong — re-run Steps 1–2. (Use `printf`, not `echo`; the shell's `echo` does not interpret `\x` escapes portably.)

- [ ] **Step 4: Write the integration test**

Create `crates/huidu-proto/tests/golden.rs`:

```rust
//! Golden-fixture round-trip tests: decode captured bytes, and re-encode to
//! confirm the codec reproduces them byte-for-byte.

use bytes::{Bytes, BytesMut};
use huidu_proto::{CmdCode, HuiduCodec, OwnedFrame, SdkFragment};
use std::path::Path;
use tokio_util::codec::{Decoder, Encoder};

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn heartbeat_golden_decodes() {
    let mut codec = HuiduCodec;
    let mut buf = BytesMut::from(&fixture("heartbeat.bin")[..]);
    let frame = codec.decode(&mut buf).unwrap().unwrap();
    assert_eq!(frame.cmd, CmdCode::Heartbeat);
    assert!(frame.payload.is_empty());
    assert!(buf.is_empty());
}

#[test]
fn heartbeat_golden_reencodes() {
    let golden = fixture("heartbeat.bin");
    let mut codec = HuiduCodec;
    let mut out = BytesMut::new();
    codec
        .encode(OwnedFrame::new(CmdCode::Heartbeat, Bytes::new()), &mut out)
        .unwrap();
    assert_eq!(&out[..], &golden[..]);
}

#[test]
fn sdk_hello_golden_decodes_and_parses() {
    let mut codec = HuiduCodec;
    let mut buf = BytesMut::from(&fixture("sdk_hello.bin")[..]);
    let frame = codec.decode(&mut buf).unwrap().unwrap();
    assert_eq!(frame.cmd, CmdCode::SdkCmd);
    let frag = SdkFragment::parse(&frame.payload).unwrap();
    assert_eq!(frag.total_len, 5);
    assert_eq!(frag.offset, 0);
    assert_eq!(&frag.xml_chunk[..], b"hello");
}
```

- [ ] **Step 5: Run the integration test**

Run: `cargo test -p huidu-proto --test golden`
Expected: PASS — `heartbeat_golden_decodes ... ok`, `heartbeat_golden_reencodes ... ok`, `sdk_hello_golden_decodes_and_parses ... ok`.

- [ ] **Step 6: Commit**

```bash
git add crates/huidu-proto/tests/fixtures/heartbeat.bin \
        crates/huidu-proto/tests/fixtures/heartbeat.txt \
        crates/huidu-proto/tests/fixtures/sdk_hello.bin \
        crates/huidu-proto/tests/fixtures/sdk_hello.txt \
        crates/huidu-proto/tests/golden.rs
git commit -m "test(proto): add golden frame fixtures and round-trip tests"
```

---

### Task 7: Final gate

**Files:**
- None (verification only)

- [ ] **Step 1: Full test run**

Run: `cargo test -p huidu-proto`
Expected: PASS — every unit test (frame, codec, sdk_frame) and the three golden integration tests report `ok`; final line `test result: ok.`

- [ ] **Step 2: Lint clean**

Run: `cargo clippy -p huidu-proto --all-targets -- -D warnings`
Expected: PASS — no warnings. If clippy is not installed, `rustup component add clippy` first.

- [ ] **Step 3: Docs build**

Run: `cargo doc -p huidu-proto --no-deps`
Expected: PASS — no broken intra-doc links, no warnings.

- [ ] **Step 4: Confirm the public surface**

Run: `cargo doc -p huidu-proto --no-deps` then confirm the crate re-exports exactly: `CmdCode`, `OwnedFrame`, `HuiduCodec`, `ProtoError`, `SdkFragment`, `SdkReassembler`. These are the symbols subsystems 2–8 depend on.

- [ ] **Step 5: Commit any lint/doc fixes (if Steps 2–3 required changes)**

```bash
git add -A
git commit -m "chore(proto): clippy and doc cleanup"
```

---

## What this plan deliberately leaves out

These are the remaining subsystems from `DESIGN.md §12`, each its own future `writing-plans` input. Plans 5–8 can proceed in parallel once 1–4 land.

2. **`huidu-proto` SDK messages** — the 24 XML request/response types (`quick-xml` + `serde`), `SdkResult`, adds `ProtoError::{Xml, SdkError}`.
3. **`huidu-proto` HD2020** — HD2020 Gen6 framing, bitmap encoding, bundled Plan 9 bitmap font, adds `ProtoError::Hd2020` and HD2020 `CmdCode` variants.
4. **`huidu` transport & handshake** — `Device::connect`, 3-phase handshake, heartbeat task, `close`.
5. **`huidu` SDK command surface** — `get_device_info`, `set_brightness`, network, time, boot logo, TCP server config, file listing.
6. **`huidu` screen builder & send** — `Screen`/`Program`/`Area`/`Item`, `send_screen`.
7. **`huidu` file upload** — MD5 streaming, chunked upload state machine, progress `Stream`.
8. **`huidu` HD2020 dispatch** — protocol probing at connect, HD2020-specific routing.

---

## Self-review

- **Spec coverage (subsystem 1 scope):** raw frame codec → Tasks 2–3; SDK fragment reassembly → Tasks 4–5; error types → Task 1 (grown incrementally, as noted); first golden fixtures → Task 6. All four bullets of `DESIGN.md §12` item 1 are covered. Out-of-scope `DESIGN.md` sections (§4–§11) belong to later plans and are listed above.
- **Placeholder scan:** every code and command step contains complete, runnable content; no `TODO`/`TBD`/"add error handling"/"similar to Task N".
- **Type consistency:** `CmdCode::from_u16`/`as_u16`, `OwnedFrame::new`, `HuiduCodec`, `SdkFragment { total_len, offset, xml_chunk }`, `SdkFragment::parse`/`encode`, `SdkReassembler::new`/`push`, and `ProtoError::{ShortFrame, UnknownCmd, FrameTooLarge, FragmentGap, FragmentOverflow}` are named identically everywhere they appear across tasks and tests. The fixture byte counts (4 and 17 bytes) match the `len = cmd + payload` convention used by the codec.
