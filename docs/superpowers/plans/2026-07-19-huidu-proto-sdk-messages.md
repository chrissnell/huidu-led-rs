# `huidu-proto` SDK Messages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the SDK 2.0 XML command layer to `huidu-proto` — the `<sdk guid><in method>…</in></sdk>` request / `<sdk guid><out method result>…</out></sdk>` reply envelope, an `SdkResult` type, the `SdkMethod` name set, and typed request/response bodies for all 24 SDK 2.0 methods, each with a round-trip `encode → decode → assert_eq` test. No I/O.

**Architecture:** A new `sdk` module tree under `crates/huidu-proto/src/sdk/`, built on the merged Subsystem 1 core (`frame`, `codec`, `sdk_frame`). The envelope is encoded and decoded with `quick-xml`'s event `Writer`/`Reader` (no string concatenation, correct escaping). Message bodies each expose `write_body` (emit their elements into the shared writer) and `parse` (scan the reply for their elements). Info structs that back both a setter and a getter (ethernet, wifi, time, luminance, switch-time, server, boot-logo) are shared between the request and response direction, so one struct round-trips both ways.

**Design deviation from `DESIGN.md §3.4` (serde derive):** the SDK bodies are *not* modeled with `serde` derives. Two protocol facts make event-based `quick-xml` the correct choice: (1) most request/response bodies are **multiple sibling elements with no wrapping element** under `<in>`/`<out>` (e.g. `SetTimeInfo` emits `<timezone/><summer/><sync/><time/>`), which serde cannot serialize without an artificial wrapper tag; (2) the exact wrappers a real device puts around *response* bodies are not fully known, so a lenient element-scanning parser (mirroring the proven Go reference) is safer than a strict serde structure that breaks on an unexpected wrapper. The DESIGN's hard requirements — `quick-xml` (not string concat), typed bodies, `SdkResult`, `ProtoError::{Xml, SdkError}`, round-trip tests — are all met.

**Scope boundary with Subsystem 6 (`huidu` screen builder):** the `Screen → Program → Area → Item` tree and its serialization live in Subsystem 6. This subsystem provides the `AddProgram/UpdateProgram/DeleteProgram/GetProgram` *method envelopes* only; the program bodies carry the screen payload as an opaque pre-built XML fragment.

**Tech Stack:** Rust (edition 2021), `quick-xml` (event Reader/Writer), `bytes`, `thiserror`.

## Modules & the 24 methods

- `sdk::result` — `SdkResult` (result string, `is_success`, known-code constants).
- `sdk::method` — `SdkMethod` enum, `as_str`/`from_str`, covering the SDK method-name set.
- `sdk::envelope` — `encode_request`, `decode_reply`, `SdkReply`, plus the `XmlWriter`/attribute helpers.
- `sdk::messages::device_info` — GetDeviceInfo (1).
- `sdk::messages::network` — GetEth0Info, SetEth0Info, GetWifiInfo, SetWifiInfo (2–5).
- `sdk::messages::program` — AddProgram, UpdateProgram, DeleteProgram, GetProgram (6–9).
- `sdk::messages::time` — GetTimeInfo, SetTimeInfo (10–11).
- `sdk::messages::luminance` — GetLuminancePloy, SetLuminancePloy (12–13).
- `sdk::messages::switch_time` — GetSwitchTime, SetSwitchTime (14–15).
- `sdk::messages::files` — GetFiles, DeleteFiles (16–17).
- `sdk::messages::boot_logo` — GetBootLogo, SetBootLogoName, ClearBootLogo (18–20).
- `sdk::messages::server` — GetSDKTcpServer, SetSDKTcpServer (21–22).
- `sdk::messages::screen` — OpenScreen, CloseScreen (23–24).

## Tasks

- [ ] Add `quick-xml` dependency; grow `ProtoError` with `Xml` and `SdkError` variants.
- [ ] `sdk::result` — `SdkResult` with round-trip.
- [ ] `sdk::method` — `SdkMethod` name set with `as_str`/`from_str` round-trip.
- [ ] `sdk::envelope` — encode/decode + `XmlWriter` helper; envelope round-trip tests.
- [ ] Message modules, one per group, each with per-method round-trip `encode → decode → assert_eq` tests.
- [ ] `lib.rs` re-exports; `cargo test` green; `cargo clippy` clean.
