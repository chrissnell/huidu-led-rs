# huidu SDK Command Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add typed async command wrappers to `huidu::Device` for the non-screen SDK 2.0 methods — device info, brightness, network, time, switch-time, boot logo, TCP server, and file listing/delete — each building a request from the Subsystem-2 XML types, round-tripping over the transport, and parsing a typed reply.

**Architecture:** A new `crates/huidu/src/commands.rs` module hangs an `impl Device` block off the existing `Device`. Two private helpers capture the two command shapes: `get_body` (send a body-less/typed request, decode a typed reply body) and `exec` (send a setter/action request, verify the device reported success). Every public method is a ~3-line wrapper over one of the two helpers plus a proto message type. The proto types (`EthernetInfo`, `LuminanceInfo`, …) are re-exported from the `huidu` crate so callers never reach into `huidu_proto::sdk::messages`.

**Tech Stack:** Rust, `tokio`, the existing `huidu_proto` SDK message layer (`SdkMessage`/`SdkReplyBody` traits), `bytes`. Tier-2 tests run against the scripted `common::MockDevice` already used by the handshake tests.

---

## File Structure

- Create: `crates/huidu/src/commands.rs` — the `impl Device` command surface and its two private round-trip helpers.
- Modify: `crates/huidu/src/lib.rs` — add `mod commands;` and re-export the proto request/reply types callers pass and receive.
- Modify: `crates/huidu/src/device.rs` — drop the two `#[allow(dead_code)]` markers on `send_sdk`/`DeviceInner::send_sdk` (now consumed).
- Create: `crates/huidu/tests/commands.rs` — Tier-2 command-flow tests against `MockDevice`.

The helpers rely on the fact that both `SdkMessage::decode` and `SdkReplyBody::decode` share the signature `fn(&[u8]) -> Result<Self, ProtoError>`, so `get_body` takes the decoder as a `fn` pointer (`DeviceInfo::decode`, `EthernetInfo::decode`, …) and works uniformly across both traits without a blanket impl (which would violate coherence).

---

### Task 1: Round-trip helpers and `get_device_info`

**Files:**
- Create: `crates/huidu/src/commands.rs`
- Modify: `crates/huidu/src/lib.rs`
- Modify: `crates/huidu/src/device.rs:158,166` (remove `#[allow(dead_code)]`)
- Test: `crates/huidu/tests/commands.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/huidu/tests/commands.rs`:

```rust
//! Tier-2 command-surface flow tests against the scripted [`common::MockDevice`].

mod common;

use common::MockDevice;
use huidu::{Device, DeviceConfig, DeviceInfo};
use huidu_proto::{SdkMethod, SdkReplyBody, SdkResult};

async fn connect(mock: &MockDevice) -> Device {
    Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake")
}

#[tokio::test]
async fn get_device_info_fetches_fresh() {
    let live = DeviceInfo {
        model: "HD-A30".into(),
        device_id: "HDA30-0002".into(),
        screen_width: 256,
        screen_height: 128,
        ..Default::default()
    };
    let reply = live.clone();
    let mock = MockDevice::builder()
        .respond(SdkMethod::GetDeviceInfo, move |req| {
            let guid = req.guid.clone().unwrap_or_default();
            reply.encode_reply(&guid, &SdkResult::success()).unwrap()
        })
        .spawn()
        .await;

    let device = connect(&mock).await;
    let got = device.get_device_info().await.expect("get_device_info");
    assert_eq!(got, live);
    device.close().await.expect("close");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p huidu --test commands get_device_info_fetches_fresh`
Expected: FAIL — `no method named get_device_info`.

- [ ] **Step 3: Write minimal implementation**

Create `crates/huidu/src/commands.rs`:

```rust
//! Typed async wrappers over the non-screen SDK 2.0 methods (`DESIGN.md §12`
//! item 5). Each wrapper builds a request from a [`huidu_proto`] message type,
//! round-trips it over the connection, and parses a typed reply.

use crate::device::Device;
use crate::error::Result;
use bytes::Bytes;
use huidu_proto::sdk::messages::device_info::{DeviceInfo, GetDeviceInfo};
use huidu_proto::sdk::{self};
use huidu_proto::{ProtoError, SdkMessage};

impl Device {
    /// Round-trip a request whose reply carries a typed body, decoding it with
    /// the reply type's own `decode` (works for both `SdkMessage` getters and
    /// `SdkReplyBody` reply-only types, which share the decoder signature).
    async fn get_body<T>(
        &self,
        request: Bytes,
        decode: fn(&[u8]) -> std::result::Result<T, ProtoError>,
    ) -> Result<T> {
        let reply = self.send_sdk(request).await?;
        Ok(decode(&reply)?)
    }

    /// Round-trip a setter or action and confirm the device reported success.
    /// The reply body is irrelevant; only its `result` matters.
    async fn exec(&self, request: Bytes) -> Result<()> {
        let reply = self.send_sdk(request).await?;
        sdk::decode_reply(&reply)?.check()?;
        Ok(())
    }

    /// Query fresh `GetDeviceInfo`. Unlike [`Device::info`], which returns the
    /// copy cached at handshake, this re-reads the device.
    pub async fn get_device_info(&self) -> Result<DeviceInfo> {
        let req = GetDeviceInfo.encode_request(self.guid())?;
        self.get_body(req, DeviceInfo::decode).await
    }
}
```

In `crates/huidu/src/lib.rs`, add `mod commands;` next to the other `mod` lines and make `send_sdk` reachable — it is already `pub(crate)` on `Device`, so no signature change is needed; just remove the two `#[allow(dead_code)]` attributes in `device.rs` (on `Device::send_sdk` and `DeviceInner::send_sdk`) since the module now uses them.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p huidu --test commands get_device_info_fetches_fresh`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/huidu/src/commands.rs crates/huidu/src/lib.rs crates/huidu/src/device.rs crates/huidu/tests/commands.rs
git commit -m "feat(huidu): command-surface scaffolding and get_device_info"
```

---

### Task 2: Brightness — get/set plus manual/scheduled/sensor helpers

**Files:**
- Modify: `crates/huidu/src/commands.rs`
- Modify: `crates/huidu/src/lib.rs` (re-export luminance types)
- Test: `crates/huidu/tests/commands.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/huidu/tests/commands.rs`:

```rust
use huidu::{LuminanceInfo, LuminanceItem, LuminanceMode};
use huidu_proto::sdk::SdkReply;
use std::sync::{Arc, Mutex};

/// Capture the parsed request body a setter sends, and reply success.
fn capture<T: SdkMessage + Send + 'static>(
    slot: Arc<Mutex<Option<T>>>,
) -> impl Fn(&SdkReply) -> Bytes + Send + Sync + 'static {
    move |req: &SdkReply| {
        *slot.lock().unwrap() = Some(T::parse_body(req).unwrap());
        let guid = req.guid.clone().unwrap_or_default();
        sdk::encode_reply(&guid, T::METHOD, &SdkResult::success(), |_| Ok(())).unwrap()
    }
}
```

(Add the imports `use bytes::Bytes;`, `use huidu_proto::SdkMessage;`, and `use huidu_proto::sdk;` at the top of the test file if not already present.)

```rust
#[tokio::test]
async fn get_brightness_parses_reply() {
    let live = LuminanceInfo {
        mode: LuminanceMode::Fixed,
        default_value: 55,
        ..Default::default()
    };
    let reply = live.clone();
    let mock = MockDevice::builder()
        .respond(SdkMethod::GetLuminancePloy, move |req| {
            let guid = req.guid.clone().unwrap_or_default();
            reply.encode_reply(&guid, &SdkResult::success()).unwrap()
        })
        .spawn()
        .await;
    let device = connect(&mock).await;
    let got = device.get_brightness().await.expect("get_brightness");
    assert_eq!(got, live);
    device.close().await.expect("close");
}

#[tokio::test]
async fn set_brightness_manual_sends_fixed_mode() {
    let slot = Arc::new(Mutex::new(None::<LuminanceInfo>));
    let mock = MockDevice::builder()
        .respond(SdkMethod::SetLuminancePloy, capture(slot.clone()))
        .spawn()
        .await;
    let device = connect(&mock).await;
    device.set_brightness_manual(42).await.expect("set");
    let sent = slot.lock().unwrap().clone().expect("captured");
    assert_eq!(sent.mode, LuminanceMode::Fixed);
    assert_eq!(sent.default_value, 42);
    device.close().await.expect("close");
}

#[tokio::test]
async fn set_brightness_scheduled_sends_items() {
    let slot = Arc::new(Mutex::new(None::<LuminanceInfo>));
    let mock = MockDevice::builder()
        .respond(SdkMethod::SetLuminancePloy, capture(slot.clone()))
        .spawn()
        .await;
    let device = connect(&mock).await;
    let items = vec![LuminanceItem {
        enabled: true,
        start: "06:00:00".into(),
        percent: 90,
    }];
    device
        .set_brightness_scheduled(items.clone())
        .await
        .expect("set");
    let sent = slot.lock().unwrap().clone().expect("captured");
    assert_eq!(sent.mode, LuminanceMode::Scheduled);
    assert_eq!(sent.items, items);
    device.close().await.expect("close");
}

#[tokio::test]
async fn set_brightness_sensor_sends_range() {
    let slot = Arc::new(Mutex::new(None::<LuminanceInfo>));
    let mock = MockDevice::builder()
        .respond(SdkMethod::SetLuminancePloy, capture(slot.clone()))
        .spawn()
        .await;
    let device = connect(&mock).await;
    device.set_brightness_sensor(5, 80, 15).await.expect("set");
    let sent = slot.lock().unwrap().clone().expect("captured");
    assert_eq!(sent.mode, LuminanceMode::Sensor);
    assert_eq!((sent.sensor_min, sent.sensor_max, sent.sensor_time), (5, 80, 15));
    device.close().await.expect("close");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p huidu --test commands set_brightness`
Expected: FAIL — `no method named get_brightness` / `set_brightness_manual`.

- [ ] **Step 3: Write minimal implementation**

In `crates/huidu/src/commands.rs`, add to the `use` block:

```rust
use huidu_proto::sdk::messages::luminance::{
    GetLuminance, LuminanceInfo, LuminanceItem, LuminanceMode,
};
```

and inside `impl Device`:

```rust
    /// Read the device's brightness policy.
    pub async fn get_brightness(&self) -> Result<LuminanceInfo> {
        let req = GetLuminance.encode_request(self.guid())?;
        self.get_body(req, LuminanceInfo::decode).await
    }

    /// Apply a fully-specified brightness policy.
    pub async fn set_brightness(&self, info: &LuminanceInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// Set a fixed brightness `percent` (1–100).
    pub async fn set_brightness_manual(&self, percent: u8) -> Result<()> {
        self.set_brightness(&LuminanceInfo {
            mode: LuminanceMode::Fixed,
            default_value: percent,
            ..Default::default()
        })
        .await
    }

    /// Drive brightness from a time-of-day schedule.
    pub async fn set_brightness_scheduled(&self, items: Vec<LuminanceItem>) -> Result<()> {
        self.set_brightness(&LuminanceInfo {
            mode: LuminanceMode::Scheduled,
            items,
            ..Default::default()
        })
        .await
    }

    /// Drive brightness from the ambient light sensor, clamped to
    /// `min..=max` percent and averaged over `time` seconds.
    pub async fn set_brightness_sensor(&self, min: u8, max: u8, time: u8) -> Result<()> {
        self.set_brightness(&LuminanceInfo {
            mode: LuminanceMode::Sensor,
            sensor_min: min,
            sensor_max: max,
            sensor_time: time,
            ..Default::default()
        })
        .await
    }
```

In `crates/huidu/src/lib.rs`, re-export the luminance types:

```rust
pub use huidu_proto::sdk::messages::luminance::{
    LuminanceInfo, LuminanceItem, LuminanceMode,
};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p huidu --test commands set_brightness get_brightness`
Expected: PASS (all four).

- [ ] **Step 5: Commit**

```bash
git add crates/huidu/src/commands.rs crates/huidu/src/lib.rs crates/huidu/tests/commands.rs
git commit -m "feat(huidu): brightness get/set with manual/scheduled/sensor helpers"
```

---

### Task 3: Network — Ethernet and Wi-Fi get/set

**Files:**
- Modify: `crates/huidu/src/commands.rs`
- Modify: `crates/huidu/src/lib.rs` (re-export network types)
- Test: `crates/huidu/tests/commands.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/huidu/tests/commands.rs`:

```rust
use huidu::{EthernetInfo, WifiInfo, WifiMode};

#[tokio::test]
async fn get_ethernet_parses_reply() {
    let live = EthernetInfo {
        enabled: true,
        auto_dhcp: false,
        ip: "10.0.0.5".into(),
        netmask: "255.255.255.0".into(),
        gateway: "10.0.0.1".into(),
        dns: "1.1.1.1".into(),
    };
    let reply = live.clone();
    let mock = MockDevice::builder()
        .respond(SdkMethod::GetEth0Info, move |req| {
            let guid = req.guid.clone().unwrap_or_default();
            reply.encode_reply(&guid, &SdkResult::success()).unwrap()
        })
        .spawn()
        .await;
    let device = connect(&mock).await;
    assert_eq!(device.get_ethernet().await.expect("get"), live);
    device.close().await.expect("close");
}

#[tokio::test]
async fn set_ethernet_sends_static_config() {
    let slot = Arc::new(Mutex::new(None::<EthernetInfo>));
    let mock = MockDevice::builder()
        .respond(SdkMethod::SetEth0Info, capture(slot.clone()))
        .spawn()
        .await;
    let device = connect(&mock).await;
    let cfg = EthernetInfo {
        enabled: true,
        auto_dhcp: false,
        ip: "192.168.6.1".into(),
        netmask: "255.255.255.0".into(),
        gateway: "192.168.6.254".into(),
        dns: "8.8.8.8".into(),
    };
    device.set_ethernet(&cfg).await.expect("set");
    assert_eq!(slot.lock().unwrap().clone().expect("captured"), cfg);
    device.close().await.expect("close");
}

#[tokio::test]
async fn set_wifi_sends_station_config() {
    let slot = Arc::new(Mutex::new(None::<WifiInfo>));
    let mock = MockDevice::builder()
        .respond(SdkMethod::SetWifiInfo, capture(slot.clone()))
        .spawn()
        .await;
    let device = connect(&mock).await;
    let cfg = WifiInfo {
        has_wifi: true,
        enabled: true,
        mode: WifiMode::Station,
        ssid: "office-net".into(),
        password: "s3cr3t".into(),
        channel: "6".into(),
        encryption: "WPA-PSK".into(),
    };
    device.set_wifi(&cfg).await.expect("set");
    let sent = slot.lock().unwrap().clone().expect("captured");
    assert_eq!(sent.mode, WifiMode::Station);
    assert_eq!(sent.ssid, "office-net");
    device.close().await.expect("close");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p huidu --test commands ethernet wifi`
Expected: FAIL — `no method named get_ethernet`.

- [ ] **Step 3: Write minimal implementation**

In `crates/huidu/src/commands.rs`, add to the `use` block:

```rust
use huidu_proto::sdk::messages::network::{
    EthernetInfo, GetEthernet, GetWifi, WifiInfo,
};
```

and inside `impl Device`:

```rust
    /// Read the Ethernet (`eth0`) configuration.
    pub async fn get_ethernet(&self) -> Result<EthernetInfo> {
        let req = GetEthernet.encode_request(self.guid())?;
        self.get_body(req, EthernetInfo::decode).await
    }

    /// Apply an Ethernet configuration (static or, with `auto_dhcp`, DHCP).
    pub async fn set_ethernet(&self, info: &EthernetInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// Read the Wi-Fi configuration.
    pub async fn get_wifi(&self) -> Result<WifiInfo> {
        let req = GetWifi.encode_request(self.guid())?;
        self.get_body(req, WifiInfo::decode).await
    }

    /// Apply a Wi-Fi configuration (access-point or station mode).
    pub async fn set_wifi(&self, info: &WifiInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }
```

In `crates/huidu/src/lib.rs`, re-export:

```rust
pub use huidu_proto::sdk::messages::network::{EthernetInfo, WifiInfo, WifiMode};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p huidu --test commands ethernet wifi`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/huidu/src/commands.rs crates/huidu/src/lib.rs crates/huidu/tests/commands.rs
git commit -m "feat(huidu): Ethernet and Wi-Fi get/set"
```

---

### Task 4: Time and switch-time get/set

**Files:**
- Modify: `crates/huidu/src/commands.rs`
- Modify: `crates/huidu/src/lib.rs` (re-export time and switch-time types)
- Test: `crates/huidu/tests/commands.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/huidu/tests/commands.rs`:

```rust
use huidu::{SwitchTimeInfo, SwitchTimeItem, TimeInfo};

#[tokio::test]
async fn set_time_sends_timezone_and_clock() {
    let slot = Arc::new(Mutex::new(None::<TimeInfo>));
    let mock = MockDevice::builder()
        .respond(SdkMethod::SetTimeInfo, capture(slot.clone()))
        .spawn()
        .await;
    let device = connect(&mock).await;
    let cfg = TimeInfo {
        timezone: "(UTC+03:00)Istanbul".into(),
        summer: false,
        sync: "none".into(),
        time: "2026-07-19 14:30:00".into(),
    };
    device.set_time(&cfg).await.expect("set");
    assert_eq!(slot.lock().unwrap().clone().expect("captured"), cfg);
    device.close().await.expect("close");
}

#[tokio::test]
async fn get_time_parses_reply() {
    let live = TimeInfo {
        timezone: "(UTC+00:00)UTC".into(),
        summer: true,
        sync: "network".into(),
        time: String::new(),
    };
    let reply = live.clone();
    let mock = MockDevice::builder()
        .respond(SdkMethod::GetTimeInfo, move |req| {
            let guid = req.guid.clone().unwrap_or_default();
            reply.encode_reply(&guid, &SdkResult::success()).unwrap()
        })
        .spawn()
        .await;
    let device = connect(&mock).await;
    assert_eq!(device.get_time().await.expect("get"), live);
    device.close().await.expect("close");
}

#[tokio::test]
async fn set_switch_time_sends_windows() {
    let slot = Arc::new(Mutex::new(None::<SwitchTimeInfo>));
    let mock = MockDevice::builder()
        .respond(SdkMethod::SetSwitchTime, capture(slot.clone()))
        .spawn()
        .await;
    let device = connect(&mock).await;
    let cfg = SwitchTimeInfo {
        open_enabled: true,
        ploy_enabled: true,
        items: vec![SwitchTimeItem {
            enabled: true,
            start: "08:00:00".into(),
            end: "22:00:00".into(),
        }],
    };
    device.set_switch_time(&cfg).await.expect("set");
    assert_eq!(slot.lock().unwrap().clone().expect("captured"), cfg);
    device.close().await.expect("close");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p huidu --test commands time switch`
Expected: FAIL — `no method named set_time`.

- [ ] **Step 3: Write minimal implementation**

In `crates/huidu/src/commands.rs`, add to the `use` block:

```rust
use huidu_proto::sdk::messages::switch_time::{GetSwitchTime, SwitchTimeInfo};
use huidu_proto::sdk::messages::time::{GetTime, TimeInfo};
```

and inside `impl Device`:

```rust
    /// Read the device clock, timezone, and DST configuration.
    pub async fn get_time(&self) -> Result<TimeInfo> {
        let req = GetTime.encode_request(self.guid())?;
        self.get_body(req, TimeInfo::decode).await
    }

    /// Set the clock, timezone, DST, and sync mode.
    pub async fn set_time(&self, info: &TimeInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// Read the scheduled screen on/off rules.
    pub async fn get_switch_time(&self) -> Result<SwitchTimeInfo> {
        let req = GetSwitchTime.encode_request(self.guid())?;
        self.get_body(req, SwitchTimeInfo::decode).await
    }

    /// Set the scheduled screen on/off rules.
    pub async fn set_switch_time(&self, info: &SwitchTimeInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }
```

In `crates/huidu/src/lib.rs`, re-export:

```rust
pub use huidu_proto::sdk::messages::switch_time::{SwitchTimeInfo, SwitchTimeItem};
pub use huidu_proto::sdk::messages::time::TimeInfo;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p huidu --test commands time switch`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/huidu/src/commands.rs crates/huidu/src/lib.rs crates/huidu/tests/commands.rs
git commit -m "feat(huidu): time and switch-time get/set"
```

---

### Task 5: Boot logo — get/set-name/clear

**Files:**
- Modify: `crates/huidu/src/commands.rs`
- Modify: `crates/huidu/src/lib.rs` (re-export boot-logo type)
- Test: `crates/huidu/tests/commands.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/huidu/tests/commands.rs`:

```rust
use huidu::BootLogoInfo;

#[tokio::test]
async fn get_boot_logo_parses_reply() {
    let live = BootLogoInfo {
        exists: true,
        name: "splash.png".into(),
        md5: "deadbeef".into(),
    };
    let reply = live.clone();
    let mock = MockDevice::builder()
        .respond(SdkMethod::GetBootLogo, move |req| {
            let guid = req.guid.clone().unwrap_or_default();
            reply.encode_reply(&guid, &SdkResult::success()).unwrap()
        })
        .spawn()
        .await;
    let device = connect(&mock).await;
    assert_eq!(device.get_boot_logo().await.expect("get"), live);
    device.close().await.expect("close");
}

#[tokio::test]
async fn set_boot_logo_name_sends_body() {
    let slot = Arc::new(Mutex::new(None::<BootLogoInfo>));
    let mock = MockDevice::builder()
        .respond(SdkMethod::SetBootLogoName, capture(slot.clone()))
        .spawn()
        .await;
    let device = connect(&mock).await;
    let cfg = BootLogoInfo {
        exists: true,
        name: "splash.png".into(),
        md5: "deadbeef".into(),
    };
    device.set_boot_logo_name(&cfg).await.expect("set");
    assert_eq!(slot.lock().unwrap().clone().expect("captured"), cfg);
    device.close().await.expect("close");
}

#[tokio::test]
async fn clear_boot_logo_completes() {
    let mock = MockDevice::builder()
        .respond(SdkMethod::ClearBootLogo, |req| {
            let guid = req.guid.clone().unwrap_or_default();
            sdk::encode_reply(&guid, SdkMethod::ClearBootLogo, &SdkResult::success(), |_| Ok(()))
                .unwrap()
        })
        .spawn()
        .await;
    let device = connect(&mock).await;
    device.clear_boot_logo().await.expect("clear");
    device.close().await.expect("close");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p huidu --test commands boot_logo`
Expected: FAIL — `no method named get_boot_logo`.

- [ ] **Step 3: Write minimal implementation**

In `crates/huidu/src/commands.rs`, add to the `use` block:

```rust
use huidu_proto::sdk::messages::boot_logo::{BootLogoInfo, ClearBootLogo, GetBootLogo};
```

and inside `impl Device`:

```rust
    /// Read the current boot-logo record.
    pub async fn get_boot_logo(&self) -> Result<BootLogoInfo> {
        let req = GetBootLogo.encode_request(self.guid())?;
        self.get_body(req, BootLogoInfo::decode).await
    }

    /// Point the boot logo at an already-uploaded image (by name + md5).
    pub async fn set_boot_logo_name(&self, info: &BootLogoInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// Remove the boot logo, reverting to the firmware default.
    pub async fn clear_boot_logo(&self) -> Result<()> {
        self.exec(ClearBootLogo.encode_request(self.guid())?).await
    }
```

In `crates/huidu/src/lib.rs`, re-export:

```rust
pub use huidu_proto::sdk::messages::boot_logo::BootLogoInfo;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p huidu --test commands boot_logo`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/huidu/src/commands.rs crates/huidu/src/lib.rs crates/huidu/tests/commands.rs
git commit -m "feat(huidu): boot logo get/set-name/clear"
```

---

### Task 6: TCP server config and file listing/delete

**Files:**
- Modify: `crates/huidu/src/commands.rs`
- Modify: `crates/huidu/src/lib.rs` (re-export server and file types)
- Test: `crates/huidu/tests/commands.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/huidu/tests/commands.rs`:

```rust
use huidu::{FileInfo, FileList, ServerInfo};
use huidu_proto::sdk::messages::files::DeleteFiles;

#[tokio::test]
async fn set_server_sends_host_and_port() {
    let slot = Arc::new(Mutex::new(None::<ServerInfo>));
    let mock = MockDevice::builder()
        .respond(SdkMethod::SetSdkTcpServer, capture(slot.clone()))
        .spawn()
        .await;
    let device = connect(&mock).await;
    let cfg = ServerInfo {
        host: "srv.example.com".into(),
        port: 6000,
    };
    device.set_server(&cfg).await.expect("set");
    assert_eq!(slot.lock().unwrap().clone().expect("captured"), cfg);
    device.close().await.expect("close");
}

#[tokio::test]
async fn list_files_parses_reply() {
    let live = FileList {
        files: vec![FileInfo {
            name: "logo.png".into(),
            size: 4096,
            exist_size: 4096,
            md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
            file_type: "image".into(),
        }],
    };
    let reply = live.clone();
    let mock = MockDevice::builder()
        .respond(SdkMethod::GetFiles, move |req| {
            let guid = req.guid.clone().unwrap_or_default();
            reply.encode_reply(&guid, &SdkResult::success()).unwrap()
        })
        .spawn()
        .await;
    let device = connect(&mock).await;
    assert_eq!(device.list_files().await.expect("list"), live);
    device.close().await.expect("close");
}

#[tokio::test]
async fn delete_files_sends_names() {
    let slot = Arc::new(Mutex::new(None::<DeleteFiles>));
    let mock = MockDevice::builder()
        .respond(SdkMethod::DeleteFiles, capture(slot.clone()))
        .spawn()
        .await;
    let device = connect(&mock).await;
    device.delete_files(["a.jpg", "b.mp4"]).await.expect("delete");
    let sent = slot.lock().unwrap().clone().expect("captured");
    assert_eq!(sent.names, vec!["a.jpg".to_string(), "b.mp4".to_string()]);
    device.close().await.expect("close");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p huidu --test commands server files list delete`
Expected: FAIL — `no method named set_server`.

- [ ] **Step 3: Write minimal implementation**

In `crates/huidu/src/commands.rs`, add to the `use` block:

```rust
use huidu_proto::sdk::messages::files::{DeleteFiles, FileList, GetFiles};
use huidu_proto::sdk::messages::server::{GetServer, ServerInfo};
```

and inside `impl Device`:

```rust
    /// Read the upstream SDK TCP-server target the device dials into.
    pub async fn get_server(&self) -> Result<ServerInfo> {
        let req = GetServer.encode_request(self.guid())?;
        self.get_body(req, ServerInfo::decode).await
    }

    /// Set the upstream SDK TCP-server host and port.
    pub async fn set_server(&self, info: &ServerInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// List the files stored on the device.
    pub async fn list_files(&self) -> Result<FileList> {
        let req = GetFiles.encode_request(self.guid())?;
        self.get_body(req, FileList::decode).await
    }

    /// Delete the named files from the device.
    pub async fn delete_files(
        &self,
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<()> {
        let req = DeleteFiles::new(names).encode_request(self.guid())?;
        self.exec(req).await
    }
```

In `crates/huidu/src/lib.rs`, re-export:

```rust
pub use huidu_proto::sdk::messages::files::{FileInfo, FileList};
pub use huidu_proto::sdk::messages::server::ServerInfo;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p huidu --test commands`
Expected: PASS (whole file).

- [ ] **Step 5: Commit**

```bash
git add crates/huidu/src/commands.rs crates/huidu/src/lib.rs crates/huidu/tests/commands.rs
git commit -m "feat(huidu): TCP server config and file listing/delete"
```

---

### Task 7: Full workspace verification

**Files:** none (verification only)

- [ ] **Step 1: Full build with warnings denied**

Run: `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean, no warnings. In particular the `#[allow(dead_code)]` markers removed in Task 1 must not resurface as `dead_code` errors — `send_sdk` is now used.

- [ ] **Step 2: Full test suite**

Run: `cargo test --workspace`
Expected: all pass — the existing `golden`, `hd2020_golden`, `handshake`, and proto unit tests, plus the new `commands` tests.

- [ ] **Step 3: Doc tests / doc build**

Run: `cargo doc --no-deps --workspace`
Expected: builds without warnings; the re-exported types resolve.

- [ ] **Step 4: Commit any fixups**

```bash
git add -A
git commit -m "chore(huidu): command-surface verification fixups"
```

(Skip if nothing changed.)

---

## Self-Review Notes

- **Spec coverage** (`DESIGN.md §12` item 5): `get_device_info` (T1), `set_brightness` manual/scheduled/sensor + get (T2), network Ethernet static/DHCP + Wi-Fi AP/Station (T3), time sync set-time/timezone/DST + get, plus switch-time as the other non-screen scheduling command (T4), boot logo get/set/clear (T5), TCP server config + file listing/delete (T6). Each wrapper builds the request via a Subsystem-2 XML type, `send`s, and parses a typed reply; every method has a Tier-2 `MockDevice` command-flow test.
- **Ethernet static vs DHCP** and **Wi-Fi AP vs Station** are expressed through the existing proto fields (`auto_dhcp`, `WifiMode`) rather than separate methods — the proto types already model both, so a second wrapper would be redundant (YAGNI).
- **OpenScreen/CloseScreen** are deliberately excluded — they belong to the screen builder & send subsystem (6), not the config command surface.
- **Type consistency:** helper names `get_body`/`exec` and every public method name are used identically in their tests. Re-exported type names (`LuminanceInfo`, `EthernetInfo`, `WifiMode`, `TimeInfo`, `SwitchTimeInfo`, `SwitchTimeItem`, `BootLogoInfo`, `ServerInfo`, `FileInfo`, `FileList`, `LuminanceItem`) match their `huidu_proto` definitions.
