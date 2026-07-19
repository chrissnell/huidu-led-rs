// Each integration-test binary that pulls in this module uses a different
// subset of the builder; suppress dead-code noise for the parts a given
// binary doesn't touch.
#![allow(dead_code)]

//! Tier-2 test harness: a scripted device that speaks the real wire protocol
//! (`DESIGN.md §10`). It binds `127.0.0.1:0`, accepts one connection, answers
//! the 3-phase handshake, auto-replies to heartbeats, and lets a test override
//! any SDK method's reply. Shared by every `huidu` integration test.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use huidu_proto::sdk::messages::device_info::DeviceInfo;
use huidu_proto::sdk::{self, SdkReply};
use huidu_proto::{
    CmdCode, HuiduCodec, OwnedFrame, SdkFragment, SdkMethod, SdkReassembler, SdkReplyBody,
    SdkResult,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::codec::Framed;

type Conn = Framed<TcpStream, HuiduCodec>;

/// Builds a reply document for a decoded SDK request.
pub type Responder = Arc<dyn Fn(&SdkReply) -> Bytes + Send + Sync>;

/// A running mock device. Aborts its server task on drop.
pub struct MockDevice {
    addr: SocketAddr,
    handle: JoinHandle<()>,
    heartbeats: Arc<AtomicUsize>,
}

impl MockDevice {
    /// Start configuring a mock.
    pub fn builder() -> MockBuilder {
        MockBuilder::new()
    }

    /// The address to point [`huidu::Device::connect`] at.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// How many heartbeat pings the device has answered so far.
    pub fn heartbeats(&self) -> usize {
        self.heartbeats.load(Ordering::SeqCst)
    }
}

impl Drop for MockDevice {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Configures a [`MockDevice`] before it is spawned.
pub struct MockBuilder {
    guid: String,
    info: DeviceInfo,
    /// Command code the version phase replies with; `None` reads the ask then
    /// drops the connection, to exercise a phase-1 failure.
    version_reply: Option<CmdCode>,
    /// Read `VersionAsk` then hang forever without replying, to exercise a
    /// phase-1 timeout.
    stall_version: bool,
    /// After the handshake, drop the connection on the next heartbeat instead
    /// of answering, to exercise the heartbeat-failure poison path.
    drop_on_heartbeat: bool,
    responders: Vec<(SdkMethod, Responder)>,
}

impl MockBuilder {
    fn new() -> Self {
        Self {
            guid: "session-0001".into(),
            info: DeviceInfo::default(),
            version_reply: Some(CmdCode::VersionReply),
            stall_version: false,
            drop_on_heartbeat: false,
            responders: Vec::new(),
        }
    }

    /// Set the session guid the handshake hands back.
    pub fn guid(mut self, guid: impl Into<String>) -> Self {
        self.guid = guid.into();
        self
    }

    /// Set the `GetDeviceInfo` the handshake caches.
    pub fn info(mut self, info: DeviceInfo) -> Self {
        self.info = info;
        self
    }

    /// Override the version-phase reply (`None` drops the connection instead).
    pub fn version_reply(mut self, cmd: Option<CmdCode>) -> Self {
        self.version_reply = cmd;
        self
    }

    /// Hang after reading `VersionAsk` without replying, to exercise a phase-1
    /// timeout.
    pub fn stall_version(mut self) -> Self {
        self.stall_version = true;
        self
    }

    /// Drop the connection on the first heartbeat rather than answering, to
    /// exercise the heartbeat-failure poison path.
    pub fn drop_on_heartbeat(mut self) -> Self {
        self.drop_on_heartbeat = true;
        self
    }

    /// Override how one SDK method is answered. Later subsystems use this to
    /// script command-flow replies.
    pub fn respond(
        mut self,
        method: SdkMethod,
        f: impl Fn(&SdkReply) -> Bytes + Send + Sync + 'static,
    ) -> Self {
        self.responders.push((method, Arc::new(f)));
        self
    }

    fn has(&self, method: SdkMethod) -> bool {
        self.responders.iter().any(|(m, _)| *m == method)
    }

    /// Bind, spawn the server task, and return a handle.
    pub async fn spawn(mut self) -> MockDevice {
        // Fill in the two handshake replies unless a test overrode them.
        if !self.has(SdkMethod::GetIfVersion) {
            let guid = self.guid.clone();
            self = self.respond(SdkMethod::GetIfVersion, move |_| {
                sdk::encode_reply(
                    &guid,
                    SdkMethod::GetIfVersion,
                    &SdkResult::success(),
                    |_| Ok(()),
                )
                .unwrap()
            });
        }
        if !self.has(SdkMethod::GetDeviceInfo) {
            let (guid, info) = (self.guid.clone(), self.info.clone());
            self = self.respond(SdkMethod::GetDeviceInfo, move |_| {
                info.encode_reply(&guid, &SdkResult::success()).unwrap()
            });
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let heartbeats = Arc::new(AtomicUsize::new(0));

        let responders = self.responders;
        let opts = ServeOpts {
            version_reply: self.version_reply,
            stall_version: self.stall_version,
            drop_on_heartbeat: self.drop_on_heartbeat,
        };
        let hb = heartbeats.clone();
        let handle = tokio::spawn(async move {
            if let Ok((sock, _)) = listener.accept().await {
                serve(Framed::new(sock, HuiduCodec), opts, responders, hb).await;
            }
        });

        MockDevice {
            addr,
            handle,
            heartbeats,
        }
    }
}

/// Behavioral switches for the server loop.
struct ServeOpts {
    version_reply: Option<CmdCode>,
    stall_version: bool,
    drop_on_heartbeat: bool,
}

/// The server loop: dispatch each frame until the client disconnects.
async fn serve(
    mut conn: Conn,
    opts: ServeOpts,
    responders: Vec<(SdkMethod, Responder)>,
    heartbeats: Arc<AtomicUsize>,
) {
    let mut reasm = SdkReassembler::new();
    while let Some(frame) = conn.next().await {
        let Ok(frame) = frame else { return };
        match frame.cmd {
            CmdCode::VersionAsk => {
                if opts.stall_version {
                    std::future::pending::<()>().await;
                }
                match opts.version_reply {
                    Some(cmd) => {
                        if conn.send(OwnedFrame::new(cmd, Bytes::new())).await.is_err() {
                            return;
                        }
                    }
                    None => return,
                }
            }
            CmdCode::Heartbeat => {
                heartbeats.fetch_add(1, Ordering::SeqCst);
                if opts.drop_on_heartbeat {
                    return;
                }
                let reply = OwnedFrame::new(CmdCode::HeartbeatReply, Bytes::new());
                if conn.send(reply).await.is_err() {
                    return;
                }
            }
            CmdCode::SdkCmd => {
                let Ok(frag) = SdkFragment::parse(&frame.payload) else {
                    return;
                };
                let xml = match reasm.push(frag) {
                    Ok(Some(xml)) => xml,
                    Ok(None) => continue,
                    Err(_) => return,
                };
                let Ok(request) = sdk::decode_reply(&xml) else {
                    return;
                };
                let Some((_, responder)) = responders.iter().find(|(m, _)| *m == request.method)
                else {
                    return;
                };
                let doc = responder(&request);
                let frag = SdkFragment {
                    total_len: doc.len() as u32,
                    offset: 0,
                    xml_chunk: doc,
                };
                if conn
                    .send(OwnedFrame::new(CmdCode::SdkReply, frag.encode()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            _ => return,
        }
    }
}
