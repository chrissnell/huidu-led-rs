//! Tier-2 `send_screen` / `send_text` flow tests against the scripted
//! [`common::MockDevice`] (`DESIGN.md §10`).

mod common;

use std::sync::{Arc, Mutex};

use common::MockDevice;
use huidu::{Area, Color, Device, DeviceConfig, DeviceInfo, Effect, Program, Screen, TextConfig};
use huidu_proto::sdk::{self};
use huidu_proto::{SdkMethod, SdkResult};

fn sample_info() -> DeviceInfo {
    DeviceInfo {
        model: "HD-A30".into(),
        device_id: "HDA30-0001".into(),
        screen_width: 128,
        screen_height: 64,
        ..Default::default()
    }
}

/// A mock that records the raw `AddProgram` request document and answers with
/// `result`. The captured document is shared back to the test.
async fn capturing_mock(result: SdkResult) -> (MockDevice, Arc<Mutex<Option<String>>>) {
    let captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let cap = captured.clone();
    let mock = MockDevice::builder()
        .info(sample_info())
        .respond(SdkMethod::AddProgram, move |req| {
            *cap.lock().unwrap() = Some(req.raw.clone());
            let guid = req.guid.clone().unwrap_or_default();
            sdk::encode_reply(&guid, SdkMethod::AddProgram, &result, |_| Ok(())).unwrap()
        })
        .spawn()
        .await;
    (mock, captured)
}

#[tokio::test]
async fn send_screen_pushes_addprogram_with_the_tree() {
    let (mock, captured) = capturing_mock(SdkResult::success()).await;
    let device = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake");

    let mut screen = Screen::new();
    let mut prog = Program::new("welcome");
    let mut area = Area::full_screen(128, 64);
    area.add_text(
        "Hello & <World>",
        TextConfig {
            color: Color::RED,
            effect: Effect::LeftScrollLoop,
            ..Default::default()
        },
    );
    prog.push_area(area);
    screen.push_program(prog);

    device.send_screen(&screen).await.expect("send_screen");

    let raw = captured
        .lock()
        .unwrap()
        .clone()
        .expect("AddProgram captured");
    assert!(raw.contains("method=\"AddProgram\""), "raw: {raw}");
    assert!(raw.contains("<screen>"), "raw: {raw}");
    assert!(raw.contains("name=\"welcome\""), "raw: {raw}");
    assert!(
        raw.contains("<string>Hello &amp; &lt;World&gt;</string>"),
        "raw: {raw}"
    );
    assert!(raw.contains("color=\"#ff0000\""), "raw: {raw}");
    assert!(raw.contains("singleLine=\"true\""), "raw: {raw}");

    device.close().await.expect("close");
}

#[tokio::test]
async fn send_text_builds_a_full_screen_program() {
    let (mock, captured) = capturing_mock(SdkResult::success()).await;
    let device = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake");

    device
        .send_text(
            "Lobby",
            TextConfig {
                color: Color::CYAN,
                ..Default::default()
            },
        )
        .await
        .expect("send_text");

    let raw = captured
        .lock()
        .unwrap()
        .clone()
        .expect("AddProgram captured");
    // Area sized to the device's cached panel (128x64 from sample_info).
    assert!(
        raw.contains("<rectangle x=\"0\" y=\"0\" width=\"128\" height=\"64\"/>"),
        "raw: {raw}"
    );
    assert!(raw.contains("<string>Lobby</string>"), "raw: {raw}");
    assert!(raw.contains("color=\"#00ffff\""), "raw: {raw}");

    device.close().await.expect("close");
}

#[tokio::test]
async fn empty_screen_sends_an_empty_screen_element() {
    let (mock, captured) = capturing_mock(SdkResult::success()).await;
    let device = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake");

    device.send_screen(&Screen::new()).await.expect("clear");

    let raw = captured
        .lock()
        .unwrap()
        .clone()
        .expect("AddProgram captured");
    assert!(raw.contains("<screen></screen>"), "raw: {raw}");

    device.close().await.expect("close");
}

#[tokio::test]
async fn device_error_reply_surfaces_as_proto_error() {
    let (mock, _captured) = capturing_mock(SdkResult::from("kParseXmlFailed")).await;
    let device = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake");

    let mut screen = Screen::new();
    let mut prog = Program::new("p");
    let mut area = Area::full_screen(64, 32);
    area.add_text("x", TextConfig::default());
    prog.push_area(area);
    screen.push_program(prog);

    let err = device
        .send_screen(&screen)
        .await
        .expect_err("device error must surface");
    assert!(
        matches!(err, huidu::Error::Proto(_)),
        "expected a proto error, got {err:?}"
    );

    device.close().await.expect("close");
}
