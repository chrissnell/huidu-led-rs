//! The connected device: `connect`, the 3-phase handshake, the background
//! heartbeat, and `close` (`DESIGN.md §4.1`–§4.4).

use crate::config::DeviceConfig;
use crate::error::{Error, ProtocolKind, Result};
use crate::transport::{self, Conn, PoisonGuard};
use bytes::Bytes;
use huidu_proto::sdk::messages::device_info::{DeviceInfo, GetDeviceInfo};
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
    /// Connect to `addr`, run the handshake, and start the heartbeat.
    ///
    /// The three phases (`DESIGN.md §4.2`) run linearly on the bare socket; any
    /// failure drops the connection and returns [`Error::Handshake`] tagged with
    /// the failing phase. No auto-reconnect.
    pub async fn connect(addr: SocketAddr, config: DeviceConfig) -> Result<Self> {
        // Bound the TCP connect by the same deadline as a round-trip, so an
        // unreachable device fails fast instead of hanging on the OS default.
        let stream = tokio::time::timeout(config.timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| Error::Timeout(config.timeout))??;
        let mut conn = Framed::new(stream, HuiduCodec);
        tracing::debug!(%addr, "connected; starting handshake");

        // Phase 1 — transport version. The VersionReply payload is unspecified
        // in DESIGN §3; v0 sends an empty ask and only asserts the reply code.
        transport::raw_roundtrip(
            &mut conn,
            OwnedFrame::new(CmdCode::VersionAsk, Bytes::new()),
            CmdCode::VersionReply,
            config.timeout,
        )
        .await
        .map_err(|e| handshake(1, e))?;

        // Phase 2 — GetIFVersion yields the session guid on the reply's <sdk>.
        let req = sdk::encode_request(PLACEHOLDER_GUID, SdkMethod::GetIfVersion, |_| Ok(()))
            .map_err(|e| handshake(2, e.into()))?;
        let reply = transport::sdk_roundtrip(&mut conn, req, &config)
            .await
            .map_err(|e| handshake(2, e))?;
        let guid = session_guid(&reply).map_err(|e| handshake(2, e))?;
        tracing::debug!(%guid, "handshake session established");

        // Phase 3 — GetDeviceInfo, cached for the life of the connection.
        let req = GetDeviceInfo
            .encode_request(&guid)
            .map_err(|e| handshake(3, e.into()))?;
        let reply = transport::sdk_roundtrip(&mut conn, req, &config)
            .await
            .map_err(|e| handshake(3, e))?;
        let info = DeviceInfo::decode(&reply).map_err(|e| handshake(3, e.into()))?;
        tracing::info!(model = %info.model, id = %info.device_id, "device ready");

        let inner = Arc::new(DeviceInner {
            frames: Mutex::new(conn),
            guid,
            info,
            config,
            protocol: ProtocolKind::Sdk2,
            heartbeat: StdMutex::new(None),
            poisoned: AtomicBool::new(false),
        });

        let handle = tokio::spawn(heartbeat_loop(
            Arc::downgrade(&inner),
            inner.config.heartbeat,
        ));
        *inner.heartbeat.lock().unwrap() = Some(handle);

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

    /// The session guid assigned during the handshake.
    pub fn guid(&self) -> &str {
        &self.inner.guid
    }

    /// The device info cached during the handshake.
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

    /// Send an SDK XML request and return the reassembled reply XML. The command
    /// surface subsystem builds typed wrappers on top of this.
    #[allow(dead_code)] // first consumed by the SDK command-surface subsystem
    pub(crate) async fn send_sdk(&self, xml: Bytes) -> Result<Bytes> {
        self.inner.send_sdk(xml).await
    }
}

impl DeviceInner {
    /// One serialized SDK round-trip, guarded against cancel-induced desync.
    #[allow(dead_code)] // first consumed by the SDK command-surface subsystem
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
