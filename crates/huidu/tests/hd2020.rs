//! Tier-2 tests for protocol probing and HD2020 dispatch (`DESIGN.md §6`).
//!
//! Covers both probe outcomes: an SDK 2.0 device classified from an empty
//! version reply, and an HD2020 Gen6 device classified from a reply led by the
//! HD2020 start byte — plus the honest cross-protocol rejection and the raw
//! HD2020 realtime-text send path wired to the subsystem-3 bitmap encoder.

mod common;

use std::net::SocketAddr;

use bytes::{Bytes, BytesMut};
use common::MockDevice;
use futures::{SinkExt, StreamExt};
use huidu::{Device, DeviceConfig, Error, ProtocolKind, TextLayout};
use huidu_proto::hd2020::{self, Hd2020Cmd, Hd2020Frame};
use huidu_proto::{CmdCode, HuiduCodec, OwnedFrame};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_util::codec::Framed;

#[tokio::test]
async fn empty_probe_classifies_as_sdk2() {
    // The default mock answers the probe with an empty payload → SDK 2.0.
    let mock = MockDevice::builder().spawn().await;
    let device = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake");
    assert_eq!(device.protocol(), ProtocolKind::Sdk2);
    device.close().await.expect("close");
}

#[tokio::test]
async fn realtime_text_rejected_on_sdk2_connection() {
    // HD2020-only commands must fail honestly on an SDK 2.0 device, before any
    // bytes hit the wire.
    let mock = MockDevice::builder().spawn().await;
    let device = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake");

    let err = device
        .send_realtime_text("HI", 32, 8, &TextLayout::default())
        .await
        .expect_err("HD2020 command must be rejected on SDK 2.0");
    assert!(
        matches!(err, Error::UnsupportedForProtocol(ProtocolKind::Sdk2)),
        "expected UnsupportedForProtocol(Sdk2), got {err:?}"
    );

    device.close().await.expect("close");
}

#[tokio::test]
async fn hd2020_probe_classifies_and_skips_sdk_phases() {
    // A version reply led by the HD2020 start byte selects the HD2020 path,
    // which runs no SDK identity phases: no guid, default info.
    let mock = MockDevice::builder()
        .version_payload(Bytes::from_static(&[hd2020::START]))
        .spawn()
        .await;

    let device = Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake");
    assert_eq!(device.protocol(), ProtocolKind::Hd2020);
    assert_eq!(device.guid(), "");

    device.close().await.expect("close");
}

#[tokio::test]
async fn hd2020_realtime_text_reaches_the_wire_as_a_native_frame() {
    let (addr, captured) = spawn_hd2020_probe_mock().await;

    let device = Device::connect(addr, DeviceConfig::default())
        .await
        .expect("handshake");
    assert_eq!(device.protocol(), ProtocolKind::Hd2020);

    device
        .send_realtime_text("HI", 32, 8, &TextLayout::default())
        .await
        .expect("realtime text send");

    let frame = captured.await.expect("mock captured a frame");
    assert_eq!(frame.cmd, Hd2020Cmd::RealtimeText);
    // Payload opens with the little-endian 32×8 panel dimensions...
    assert_eq!(&frame.payload[..4], &[32, 0, 8, 0]);
    // ...and the rasterized text lit some pixels.
    assert!(frame.payload[4..].iter().any(|&b| b != 0));

    device.close().await.expect("close");
}

/// A mock that answers the version probe as an HD2020 device, then reads the one
/// native HD2020 frame the client pushes and hands it back over `captured`.
async fn spawn_hd2020_probe_mock() -> (SocketAddr, oneshot::Receiver<Hd2020Frame>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel();

    // Detached: the task outlives its handle and drives the mock until the test
    // captures the frame.
    tokio::spawn(async move {
        let Ok((sock, _)) = listener.accept().await else {
            return;
        };

        // The probe rides the shared SDK envelope, so decode it with HuiduCodec.
        let mut framed = Framed::new(sock, HuiduCodec);
        match framed.next().await {
            Some(Ok(frame)) if frame.cmd == CmdCode::VersionAsk => {}
            _ => return,
        }
        let reply = OwnedFrame::new(CmdCode::VersionReply, Bytes::from_static(&[hd2020::START]));
        if framed.send(reply).await.is_err() {
            return;
        }

        // Everything after the probe is native HD2020 framing on the raw socket.
        let parts = framed.into_parts();
        let mut io: TcpStream = parts.io;
        let mut buf = BytesMut::from(&parts.read_buf[..]);
        let mut chunk = [0u8; 256];
        loop {
            match Hd2020Frame::decode(&buf) {
                Ok(Some((frame, _))) => {
                    let _ = tx.send(frame);
                    return;
                }
                Ok(None) => {}
                Err(_) => return,
            }
            match io.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
    });

    (addr, rx)
}
