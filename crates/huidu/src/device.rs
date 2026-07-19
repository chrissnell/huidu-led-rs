//! The connected device: `connect` (protocol probe + handshake), the background
//! heartbeat, command dispatch gating, and `close` (`DESIGN.md §4.1`–§4.4, §6).

use crate::config::DeviceConfig;
use crate::error::{Error, ProtocolKind, Result};
use crate::probe;
use crate::screen::{Area, Program, Screen, TextConfig};
use crate::transport::{self, Conn, PoisonGuard};
use bytes::Bytes;
use huidu_proto::hd2020::{self, TextLayout};
use huidu_proto::sdk::messages::device_info::{DeviceInfo, GetDeviceInfo};
use huidu_proto::sdk::messages::program::ProgramPush;
use huidu_proto::sdk::{self, PLACEHOLDER_GUID};
use huidu_proto::{
    CmdCode, HuiduCodec, OwnedFrame, ProtoError, SdkMessage, SdkMethod, SdkReplyBody,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tokio_util::codec::Framed;

/// An async client for a single Huidu LED controller.
///
/// [`Device::connect`] performs the 3-phase handshake and starts a background
/// heartbeat; commands serialize through one connection. There is no
/// auto-reconnect: a cancelled command poisons the connection (see
/// [`Error::Poisoned`]) and the caller reconnects.
pub struct Device {
    pub(crate) inner: Arc<DeviceInner>,
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("guid", &self.inner.guid)
            .field("model", &self.inner.info.model)
            .field("protocol", &self.inner.protocol)
            .field("poisoned", &self.inner.poisoned.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

pub(crate) struct DeviceInner {
    pub(crate) frames: Mutex<Conn>,
    /// Session guid from `GetIFVersion`, echoed back on every command. Kept as
    /// the raw device string so we resend exactly what the device issued.
    guid: String,
    info: DeviceInfo,
    pub(crate) config: DeviceConfig,
    protocol: ProtocolKind,
    heartbeat: StdMutex<Option<JoinHandle<()>>>,
    pub(crate) poisoned: AtomicBool,
}

impl Device {
    /// Connect to `addr`, probe the protocol, run the handshake, and (on SDK 2.0)
    /// start the heartbeat.
    ///
    /// Phase 1 sends `VersionAsk` and classifies the device from the reply
    /// (`DESIGN.md §6`): an SDK 2.0 device then runs phases 2–3 (`GetIFVersion`,
    /// `GetDeviceInfo`); an HD2020 Gen6 device skips them, as it does not speak
    /// SDK XML. Any phase failure drops the connection and returns
    /// [`Error::Handshake`] tagged with the failing phase. No auto-reconnect.
    pub async fn connect(addr: SocketAddr, config: DeviceConfig) -> Result<Self> {
        // Bound the TCP connect by the same deadline as a round-trip, so an
        // unreachable device fails fast instead of hanging on the OS default.
        let stream = tokio::time::timeout(config.timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| Error::Timeout(config.timeout))??;
        let mut conn = Framed::new(stream, HuiduCodec);
        tracing::debug!(%addr, "connected; starting handshake");

        // Phase 1 — version probe. Both protocols share this raw exchange; the
        // reply payload tells us which one the device speaks (`DESIGN.md §6`).
        let reply = transport::raw_roundtrip(
            &mut conn,
            OwnedFrame::new(CmdCode::VersionAsk, Bytes::new()),
            CmdCode::VersionReply,
            config.timeout,
        )
        .await
        .map_err(|e| handshake(1, e))?;
        let protocol = probe::classify(&reply.payload);
        tracing::debug!(?protocol, "protocol probed");

        // The SDK 2.0 identity phases only exist on the SDK path; an HD2020 Gen6
        // controller does not speak SDK XML, so it skips them and carries no
        // session guid or cached DeviceInfo in v0.
        let (guid, info) = match protocol {
            ProtocolKind::Sdk2 => sdk_handshake(&mut conn, &config).await?,
            ProtocolKind::Hd2020 => (String::new(), DeviceInfo::default()),
        };

        let inner = Arc::new(DeviceInner {
            frames: Mutex::new(conn),
            guid,
            info,
            config,
            protocol,
            heartbeat: StdMutex::new(None),
            poisoned: AtomicBool::new(false),
        });

        // The SDK heartbeat rides the shared `<len><cmd>` envelope; an HD2020
        // connection speaks native framing after the probe (`DESIGN.md §6`), so
        // interleaving SDK heartbeats there would corrupt the stream. v0 runs no
        // heartbeat on HD2020.
        if protocol == ProtocolKind::Sdk2 {
            let handle = tokio::spawn(heartbeat_loop(
                Arc::downgrade(&inner),
                inner.config.heartbeat,
            ));
            *inner.heartbeat.lock().unwrap() = Some(handle);
        }

        Ok(Device { inner })
    }

    /// Cancel the heartbeat and shut the connection down.
    pub async fn close(self) -> Result<()> {
        // Take the handle out and drop the std-mutex guard before awaiting.
        let handle = self.inner.heartbeat.lock().unwrap().take();
        if let Some(handle) = handle {
            handle.abort();
            let _ = handle.await; // the JoinError from the abort is expected
        }
        let mut frames = self.inner.frames.lock().await;
        use futures::SinkExt;
        let _ = frames.close().await; // best-effort TCP shutdown
        tracing::debug!("connection closed");
        Ok(())
    }

    /// The session guid assigned during the SDK 2.0 handshake. Empty on an
    /// [`ProtocolKind::Hd2020`] connection, which runs no SDK identity phases.
    pub fn guid(&self) -> &str {
        &self.inner.guid
    }

    /// The device info cached during the SDK 2.0 handshake. Default-valued on an
    /// [`ProtocolKind::Hd2020`] connection, which has no SDK `GetDeviceInfo`.
    pub fn info(&self) -> &DeviceInfo {
        &self.inner.info
    }

    /// The wire protocol this connection speaks.
    pub fn protocol(&self) -> ProtocolKind {
        self.inner.protocol
    }

    /// Whether the connection has been poisoned by a cancelled or failed
    /// round-trip (`DESIGN.md §4.4`). Once poisoned, every command returns
    /// [`Error::Poisoned`] and the caller must reconnect.
    pub fn is_poisoned(&self) -> bool {
        self.inner.poisoned.load(Ordering::SeqCst)
    }

    /// Push a [`Screen`] to the device, replacing all current programs
    /// (`DESIGN.md §5`).
    ///
    /// The tree is serialized to an SDK 2.0 `AddProgram` request and sent over
    /// the connection; any device-side error on the reply surfaces as
    /// [`Error::Proto`]. Only SDK 2.0 connections carry programs — an HD2020
    /// connection returns [`Error::UnsupportedForProtocol`].
    pub async fn send_screen(&self, screen: &Screen) -> Result<()> {
        if self.inner.protocol != ProtocolKind::Sdk2 {
            return Err(Error::UnsupportedForProtocol(self.inner.protocol));
        }
        let fragment = screen.to_program_xml()?;
        let request = ProgramPush::add(fragment).encode(&self.inner.guid)?;
        let reply = self.inner.send_sdk(request).await?;
        sdk::decode_reply(&reply)?.check()?;
        Ok(())
    }

    /// Convenience wrapper: show a single line of `text` full-screen with
    /// `config`, replacing all current programs.
    ///
    /// Builds a one-program, one-area [`Screen`] sized to the device's panel and
    /// delegates to [`Device::send_screen`].
    pub async fn send_text(&self, text: &str, config: TextConfig) -> Result<()> {
        let mut area =
            Area::full_screen(self.inner.info.screen_width, self.inner.info.screen_height);
        area.add_text(text, config);
        let mut program = Program::new("text");
        program.push_area(area);
        let mut screen = Screen::new();
        screen.push_program(program);
        self.send_screen(&screen).await
    }

    /// Send an SDK XML request and return the reassembled reply XML. The command
    /// surface (see `commands.rs`) builds typed wrappers on top of this.
    ///
    /// Returns [`Error::UnsupportedForProtocol`] on an [`ProtocolKind::Hd2020`]
    /// connection — HD2020 Gen6 controllers do not speak SDK 2.0 (`DESIGN.md §6`).
    pub(crate) async fn send_sdk(&self, xml: Bytes) -> Result<Bytes> {
        self.inner.require(ProtocolKind::Sdk2)?;
        self.inner.send_sdk(xml).await
    }

    /// Render `text` into a `width × height` monochrome panel and push it as an
    /// HD2020 Gen6 realtime-text frame (`DESIGN.md §6`), rasterized by the
    /// subsystem-3 bitmap encoder.
    ///
    /// Returns [`Error::UnsupportedForProtocol`] on an [`ProtocolKind::Sdk2`]
    /// connection. v0 realtime pushes are fire-and-forget: the frame is written
    /// and flushed, with no reply awaited.
    pub async fn send_realtime_text(
        &self,
        text: &str,
        width: usize,
        height: usize,
        layout: &TextLayout,
    ) -> Result<()> {
        self.inner.require(ProtocolKind::Hd2020)?;
        let frame = hd2020::realtime_text_frame(text, width, height, layout)?;
        self.inner.send_hd2020(frame).await
    }
}

impl DeviceInner {
    /// Gate a command on the protocol the connection actually speaks
    /// (`DESIGN.md §6`). Commands exclusive to the other protocol fail with
    /// [`Error::UnsupportedForProtocol`] rather than misbehaving on the wire.
    fn require(&self, kind: ProtocolKind) -> Result<()> {
        ensure_protocol(self.protocol, kind)
    }

    /// Write one native HD2020 frame under the connection mutex, guarded the same
    /// way as an SDK round-trip.
    async fn send_hd2020(&self, frame: Bytes) -> Result<()> {
        let mut frames = self.frames.lock().await;
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(Error::Poisoned);
        }
        let mut guard = PoisonGuard::new(&self.poisoned);
        transport::hd2020_send(&mut frames, frame, self.config.timeout).await?;
        guard.disarm();
        Ok(())
    }

    /// One serialized SDK round-trip, guarded against cancel-induced desync.
    async fn send_sdk(&self, xml: Bytes) -> Result<Bytes> {
        let mut frames = self.frames.lock().await;
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(Error::Poisoned);
        }
        let mut guard = PoisonGuard::new(&self.poisoned);
        let reply = transport::sdk_roundtrip(&mut frames, xml, &self.config).await?;
        guard.disarm();
        Ok(reply)
    }

    /// One heartbeat ping/reply, guarded the same way as a command.
    async fn send_heartbeat(&self) -> Result<()> {
        let mut frames = self.frames.lock().await;
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(Error::Poisoned);
        }
        let mut guard = PoisonGuard::new(&self.poisoned);
        transport::raw_roundtrip(
            &mut frames,
            OwnedFrame::new(CmdCode::Heartbeat, Bytes::new()),
            CmdCode::HeartbeatReply,
            self.config.timeout,
        )
        .await?;
        guard.disarm();
        Ok(())
    }
}

/// The SDK 2.0 identity phases (2 and 3 of `DESIGN.md §4.2`), run only when the
/// probe classified the device as [`ProtocolKind::Sdk2`]. Returns the session
/// guid and the cached [`DeviceInfo`].
async fn sdk_handshake(conn: &mut Conn, config: &DeviceConfig) -> Result<(String, DeviceInfo)> {
    // Phase 2 — GetIFVersion yields the session guid on the reply's <sdk>.
    let req = sdk::encode_request(PLACEHOLDER_GUID, SdkMethod::GetIfVersion, |_| Ok(()))
        .map_err(|e| handshake(2, e.into()))?;
    let reply = transport::sdk_roundtrip(conn, req, config)
        .await
        .map_err(|e| handshake(2, e))?;
    let guid = session_guid(&reply).map_err(|e| handshake(2, e))?;
    tracing::debug!(%guid, "handshake session established");

    // Phase 3 — GetDeviceInfo, cached for the life of the connection.
    let req = GetDeviceInfo
        .encode_request(&guid)
        .map_err(|e| handshake(3, e.into()))?;
    let reply = transport::sdk_roundtrip(conn, req, config)
        .await
        .map_err(|e| handshake(3, e))?;
    let info = DeviceInfo::decode(&reply).map_err(|e| handshake(3, e.into()))?;
    tracing::info!(model = %info.model, id = %info.device_id, "device ready");
    Ok((guid, info))
}

/// Gate a command on the protocol the connection speaks (`DESIGN.md §6`): a
/// command exclusive to the other protocol fails with
/// [`Error::UnsupportedForProtocol`] carrying the connection's actual protocol,
/// rather than misbehaving on the wire.
fn ensure_protocol(current: ProtocolKind, need: ProtocolKind) -> Result<()> {
    if current == need {
        Ok(())
    } else {
        Err(Error::UnsupportedForProtocol(current))
    }
}

/// Wrap a phase failure with its 1-based phase number.
fn handshake(phase: u8, source: Error) -> Error {
    Error::Handshake {
        phase,
        source: Box::new(source),
    }
}

/// Pull the session guid out of a `GetIFVersion` reply, surfacing any device
/// error first.
fn session_guid(reply_xml: &[u8]) -> Result<String> {
    let reply = sdk::decode_reply(reply_xml)?;
    reply.check()?;
    reply.guid.ok_or_else(|| {
        Error::Proto(ProtoError::Xml(
            "GetIFVersion reply carried no session guid".into(),
        ))
    })
}

/// Background heartbeat (`DESIGN.md §4.3`). Holds a `Weak` so it never keeps the
/// device alive; exits and poisons the connection on the first failed ping.
async fn heartbeat_loop(inner: Weak<DeviceInner>, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await; // the first tick is immediate; skip it so the first ping waits one interval
    loop {
        ticker.tick().await;
        let Some(inner) = inner.upgrade() else { return };
        if inner.poisoned.load(Ordering::SeqCst) {
            return;
        }
        if let Err(e) = inner.send_heartbeat().await {
            tracing::warn!(error = %e, "heartbeat failed; poisoning connection");
            inner.poisoned.store(true, Ordering::SeqCst);
            return;
        }
        tracing::trace!("heartbeat ok");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_protocol_allows_the_matching_protocol() {
        assert!(ensure_protocol(ProtocolKind::Sdk2, ProtocolKind::Sdk2).is_ok());
        assert!(ensure_protocol(ProtocolKind::Hd2020, ProtocolKind::Hd2020).is_ok());
    }

    #[test]
    fn ensure_protocol_rejects_sdk_command_on_hd2020() {
        let err = ensure_protocol(ProtocolKind::Hd2020, ProtocolKind::Sdk2).unwrap_err();
        assert!(matches!(
            err,
            Error::UnsupportedForProtocol(ProtocolKind::Hd2020)
        ));
    }

    #[test]
    fn ensure_protocol_rejects_hd2020_command_on_sdk() {
        let err = ensure_protocol(ProtocolKind::Sdk2, ProtocolKind::Hd2020).unwrap_err();
        assert!(matches!(
            err,
            Error::UnsupportedForProtocol(ProtocolKind::Sdk2)
        ));
    }
}
