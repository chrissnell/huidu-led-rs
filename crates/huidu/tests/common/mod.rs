// Shared across integration-test binaries; not every binary exercises every
// builder knob, so cross-binary dead-code false positives are expected here.
#![allow(dead_code)]

//! Tier-2 test harness: a scripted device that speaks the real wire protocol
//! (`DESIGN.md §10`). It binds `127.0.0.1:0`, accepts one connection, answers
//! the 3-phase handshake, auto-replies to heartbeats, and lets a test override
//! any SDK method's reply. Shared by every `huidu` integration test.
//!
//! `#![allow(dead_code)]` (above) because each integration test binary compiles
//! this module independently; a given test uses only the knobs it needs, so the
//! others read as dead code in that binary.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use huidu_proto::sdk::messages::device_info::DeviceInfo;
use huidu_proto::sdk::{self, SdkReply};
use huidu_proto::{
    CmdCode, FileContentAsk, FileContentReply, FileEndReply, FileStartAsk, FileStartReply,
    HuiduCodec, OwnedFrame, SdkFragment, SdkMethod, SdkReassembler, SdkReplyBody, SdkResult,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::codec::Framed;

type Conn = Framed<TcpStream, HuiduCodec>;

/// Builds a reply document for a decoded SDK request.
pub type Responder = Arc<dyn Fn(&SdkReply) -> Bytes + Send + Sync>;

/// What the mock recorded from a file-upload flow, for test assertions.
#[derive(Debug, Default, Clone)]
pub struct UploadCapture {
    /// The `FileStartAsk` was seen.
    pub started: bool,
    pub name: String,
    pub file_type: String,
    pub declared_size: u64,
    pub start_md5: [u8; 16],
    /// Offset of the first content chunk (the client's resume point).
    pub first_offset: Option<u64>,
    /// Chunk bytes in receive order (only counted attempts that were accepted).
    pub received: Vec<u8>,
    /// Total `FileContentAsk` frames seen, including retried ones.
    pub content_attempts: usize,
    /// The `FileEndAsk` MD5, once the transfer finished.
    pub end_md5: Option<[u8; 16]>,
}

/// A running mock device. Aborts its server task on drop.
pub struct MockDevice {
    addr: SocketAddr,
    handle: JoinHandle<()>,
    heartbeats: Arc<AtomicUsize>,
    upload: Arc<Mutex<UploadCapture>>,
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

    /// A snapshot of what the upload flow recorded.
    pub fn upload(&self) -> UploadCapture {
        self.upload.lock().unwrap().clone()
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
    /// Payload carried on the `VersionReply` frame. The client classifies the
    /// protocol from its lead byte (`DESIGN.md §6`); empty stays on the SDK 2.0
    /// path.
    version_payload: Bytes,
    /// Read `VersionAsk` then hang forever without replying, to exercise a
    /// phase-1 timeout.
    stall_version: bool,
    /// After the handshake, drop the connection on the next heartbeat instead
    /// of answering, to exercise the heartbeat-failure poison path.
    drop_on_heartbeat: bool,
    /// Resume offset reported in `FileStartReply` (bytes the device claims to
    /// already hold).
    upload_resume_offset: u64,
    /// Non-zero to reject `FileStartAsk`.
    upload_start_result: u16,
    /// Reject the first N `FileContentAsk` frames with a retryable code before
    /// accepting, to exercise the chunk-retry path.
    upload_content_fail_times: usize,
    responders: Vec<(SdkMethod, Responder)>,
}

impl MockBuilder {
    fn new() -> Self {
        Self {
            guid: "session-0001".into(),
            info: DeviceInfo::default(),
            version_reply: Some(CmdCode::VersionReply),
            version_payload: Bytes::new(),
            stall_version: false,
            drop_on_heartbeat: false,
            upload_resume_offset: 0,
            upload_start_result: 0,
            upload_content_fail_times: 0,
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

    /// Set the payload the `VersionReply` carries, which the client uses to probe
    /// the protocol (`DESIGN.md §6`).
    pub fn version_payload(mut self, payload: impl Into<Bytes>) -> Self {
        self.version_payload = payload.into();
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

    /// Report `offset` as the resume point in `FileStartReply`.
    pub fn upload_resume_offset(mut self, offset: u64) -> Self {
        self.upload_resume_offset = offset;
        self
    }

    /// Reject `FileStartAsk` with a non-zero result code.
    pub fn upload_start_result(mut self, code: u16) -> Self {
        self.upload_start_result = code;
        self
    }

    /// Reject the first `n` content chunks with a retryable code, then accept.
    pub fn upload_content_fail_times(mut self, n: usize) -> Self {
        self.upload_content_fail_times = n;
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
        let upload = Arc::new(Mutex::new(UploadCapture::default()));

        let responders = self.responders;
        let opts = ServeOpts {
            version_reply: self.version_reply,
            version_payload: self.version_payload,
            stall_version: self.stall_version,
            drop_on_heartbeat: self.drop_on_heartbeat,
            upload_resume_offset: self.upload_resume_offset,
            upload_start_result: self.upload_start_result,
            upload_content_fail_times: self.upload_content_fail_times,
        };
        let hb = heartbeats.clone();
        let up = upload.clone();
        let handle = tokio::spawn(async move {
            if let Ok((sock, _)) = listener.accept().await {
                serve(Framed::new(sock, HuiduCodec), opts, responders, hb, up).await;
            }
        });

        MockDevice {
            addr,
            handle,
            heartbeats,
            upload,
        }
    }
}

/// Behavioral switches for the server loop.
struct ServeOpts {
    version_reply: Option<CmdCode>,
    version_payload: Bytes,
    stall_version: bool,
    drop_on_heartbeat: bool,
    upload_resume_offset: u64,
    upload_start_result: u16,
    upload_content_fail_times: usize,
}

/// The server loop: dispatch each frame until the client disconnects.
async fn serve(
    mut conn: Conn,
    opts: ServeOpts,
    responders: Vec<(SdkMethod, Responder)>,
    heartbeats: Arc<AtomicUsize>,
    upload: Arc<Mutex<UploadCapture>>,
) {
    let mut reasm = SdkReassembler::new();
    let mut content_failures_left = opts.upload_content_fail_times;
    while let Some(frame) = conn.next().await {
        let Ok(frame) = frame else { return };
        match frame.cmd {
            CmdCode::VersionAsk => {
                if opts.stall_version {
                    std::future::pending::<()>().await;
                }
                match opts.version_reply {
                    Some(cmd) => {
                        let reply = OwnedFrame::new(cmd, opts.version_payload.clone());
                        if conn.send(reply).await.is_err() {
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
            CmdCode::FileStartAsk => {
                let Ok(ask) = FileStartAsk::parse(&frame.payload) else {
                    return;
                };
                {
                    let mut cap = upload.lock().unwrap();
                    cap.started = true;
                    cap.name = ask.name;
                    cap.file_type = ask.file_type;
                    cap.declared_size = ask.size;
                    cap.start_md5 = ask.md5;
                }
                let reply = FileStartReply {
                    result: opts.upload_start_result,
                    resume_offset: opts.upload_resume_offset,
                };
                if conn
                    .send(OwnedFrame::new(CmdCode::FileStartReply, reply.encode()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            CmdCode::FileContentAsk => {
                let Ok(ask) = FileContentAsk::parse(&frame.payload) else {
                    return;
                };
                let reply = if content_failures_left > 0 {
                    // Retryable rejection: don't record the bytes.
                    content_failures_left -= 1;
                    upload.lock().unwrap().content_attempts += 1;
                    FileContentReply {
                        result: 1,
                        received: 0,
                    }
                } else {
                    let mut cap = upload.lock().unwrap();
                    cap.content_attempts += 1;
                    cap.first_offset.get_or_insert(ask.offset);
                    cap.received.extend_from_slice(&ask.data);
                    let received = ask.offset + ask.data.len() as u64;
                    FileContentReply {
                        result: 0,
                        received,
                    }
                };
                if conn
                    .send(OwnedFrame::new(CmdCode::FileContentReply, reply.encode()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            CmdCode::FileEndAsk => {
                let Ok(ask) = huidu_proto::FileEndAsk::parse(&frame.payload) else {
                    return;
                };
                upload.lock().unwrap().end_md5 = Some(ask.md5);
                let reply = FileEndReply { result: 0 };
                if conn
                    .send(OwnedFrame::new(CmdCode::FileEndReply, reply.encode()))
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
