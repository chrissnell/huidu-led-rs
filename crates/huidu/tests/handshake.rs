//! Tier-2 handshake-flow tests against the scripted [`common::MockDevice`].

mod common;

use std::time::Duration;

use common::MockDevice;
use huidu::{Device, DeviceConfig, DeviceInfo, Error, ProtocolKind};
use huidu_proto::sdk::{self};
use huidu_proto::{CmdCode, SdkMethod, SdkResult};

fn sample_info() -> DeviceInfo {
    DeviceInfo {
        model: "HD-A30".into(),
        device_id: "HDA30-0001".into(),
        device_name: "Lobby Sign".into(),
        app_version: "3.2.1".into(),
        screen_width: 128,
        screen_height: 64,
        ..Default::default()
    }
}

#[tokio::test]
async fn handshake_succeeds_and_caches_identity() {
    let info = sample_info();
    let mock = MockDevice::builder()
        .guid("sess-abc-123")
        .info(info.clone())
        .spawn()
        .await;

    let device = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake");

    assert_eq!(device.guid(), "sess-abc-123");
    assert_eq!(device.info(), &info);
    assert_eq!(device.protocol(), ProtocolKind::Sdk2);

    device.close().await.expect("close");
}

#[tokio::test]
async fn version_phase_wrong_code_fails_at_phase_1() {
    // The device answers VersionAsk with the wrong command code.
    let mock = MockDevice::builder()
        .version_reply(Some(CmdCode::Heartbeat))
        .spawn()
        .await;

    let err = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect_err("phase 1 must fail");
    assert!(
        matches!(err, Error::Handshake { phase: 1, .. }),
        "expected phase-1 handshake error, got {err:?}"
    );
}

#[tokio::test]
async fn get_if_version_device_error_fails_at_phase_2() {
    let mock = MockDevice::builder()
        .respond(SdkMethod::GetIfVersion, |_| {
            sdk::encode_reply(
                "g",
                SdkMethod::GetIfVersion,
                &SdkResult::from("kParseXmlFailed"),
                |_| Ok(()),
            )
            .unwrap()
        })
        .spawn()
        .await;

    let err = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect_err("phase 2 must fail");
    assert!(
        matches!(err, Error::Handshake { phase: 2, .. }),
        "expected phase-2 handshake error, got {err:?}"
    );
}

#[tokio::test]
async fn get_device_info_error_fails_at_phase_3() {
    let mock = MockDevice::builder()
        .respond(SdkMethod::GetDeviceInfo, |req| {
            // Echo the session guid but report a device-side failure.
            let guid = req.guid.clone().unwrap_or_default();
            sdk::encode_reply(
                &guid,
                SdkMethod::GetDeviceInfo,
                &SdkResult::from("kDeviceBusy"),
                |_| Ok(()),
            )
            .unwrap()
        })
        .spawn()
        .await;

    let err = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect_err("phase 3 must fail");
    assert!(
        matches!(err, Error::Handshake { phase: 3, .. }),
        "expected phase-3 handshake error, got {err:?}"
    );
}

#[tokio::test]
async fn heartbeat_pings_the_device() {
    let mock = MockDevice::builder().spawn().await;
    let config = DeviceConfig {
        heartbeat: Duration::from_millis(40),
        ..Default::default()
    };

    let device = Device::connect(mock.addr(), config)
        .await
        .expect("handshake");
    // Give the background loop time to fire a few pings; the mock auto-answers.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        mock.heartbeats() >= 1,
        "expected at least one heartbeat, saw {}",
        mock.heartbeats()
    );

    device.close().await.expect("close");
}
