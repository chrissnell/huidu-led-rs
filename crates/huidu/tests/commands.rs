//! Tier-2 command-surface flow tests against the scripted [`common::MockDevice`].
//!
//! Getters script a reply and assert the wrapper parses it; setters capture the
//! request body the wrapper sent and assert it round-trips intact.

mod common;

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use common::MockDevice;
use huidu::{
    BootLogoInfo, Device, DeviceConfig, DeviceInfo, EthernetInfo, FileInfo, FileList, LuminanceInfo,
    LuminanceItem, LuminanceMode, ServerInfo, SwitchTimeInfo, SwitchTimeItem, TimeInfo, WifiInfo,
    WifiMode,
};
use huidu_proto::sdk::messages::files::DeleteFiles;
use huidu_proto::sdk::{self, SdkReply};
use huidu_proto::{SdkMethod, SdkMessage, SdkReplyBody, SdkResult};

async fn connect(mock: &MockDevice) -> Device {
    Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake")
}

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

// ─── Device info ──────────────────────────────────────────────────────────────

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

// ─── Brightness ───────────────────────────────────────────────────────────────

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
    assert_eq!(
        (sent.sensor_min, sent.sensor_max, sent.sensor_time),
        (5, 80, 15)
    );
    device.close().await.expect("close");
}

// ─── Network ──────────────────────────────────────────────────────────────────

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

// ─── Time and switch-time ─────────────────────────────────────────────────────

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

// ─── Boot logo ────────────────────────────────────────────────────────────────

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
            sdk::encode_reply(
                &guid,
                SdkMethod::ClearBootLogo,
                &SdkResult::success(),
                |_| Ok(()),
            )
            .unwrap()
        })
        .spawn()
        .await;
    let device = connect(&mock).await;
    device.clear_boot_logo().await.expect("clear");
    device.close().await.expect("close");
}

// ─── TCP server and files ─────────────────────────────────────────────────────

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
    device
        .delete_files(["a.jpg", "b.mp4"])
        .await
        .expect("delete");
    let sent = slot.lock().unwrap().clone().expect("captured");
    assert_eq!(sent.names, vec!["a.jpg".to_string(), "b.mp4".to_string()]);
    device.close().await.expect("close");
}
