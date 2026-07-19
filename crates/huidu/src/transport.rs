//! Round-trip primitives over a `Framed<TcpStream, HuiduCodec>` and the
//! poison-on-cancel guard (`DESIGN.md §4.1`, §4.4).
//!
//! These operate on a bare `&mut Conn` so the same code serves both the
//! handshake — which runs on the socket before it moves behind the mutex — and
//! [`crate::device::DeviceInner`], which holds the mutex for each command.

use crate::config::DeviceConfig;
use crate::error::{Error, Result};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use huidu_proto::{CmdCode, HuiduCodec, OwnedFrame, ProtoError, SdkFragment, SdkReassembler};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_util::codec::Framed;

/// The framed transport a connected device speaks over.
pub(crate) type Conn = Framed<TcpStream, HuiduCodec>;

/// Read one frame under `deadline`, mapping a closed stream and a timeout to
/// typed errors.
async fn read_frame(conn: &mut Conn, deadline: Duration) -> Result<OwnedFrame> {
    match timeout(deadline, conn.next()).await {
        Err(_) => Err(Error::Timeout(deadline)),
        Ok(None) => Err(Error::ConnectionClosed),
        Ok(Some(frame)) => Ok(frame?),
    }
}

/// Send one raw frame and await a single reply, asserting its command code.
/// Used by the version handshake and the heartbeat.
pub(crate) async fn raw_roundtrip(
    conn: &mut Conn,
    request: OwnedFrame,
    expect: CmdCode,
    deadline: Duration,
) -> Result<OwnedFrame> {
    conn.send(request).await?;
    let reply = read_frame(conn, deadline).await?;
    if reply.cmd != expect {
        return Err(Error::UnexpectedReply {
            expected: expect.as_u16(),
            got: reply.cmd.as_u16(),
        });
    }
    Ok(reply)
}

/// Send an SDK XML document — fragmenting it into `SdkCmd` frames sized by
/// `sdk_fragment_size` — then reassemble the `SdkReply` frames back into the
/// full reply XML. Timed by `config.timeout` per reply frame.
pub(crate) async fn sdk_roundtrip(
    conn: &mut Conn,
    xml: Bytes,
    config: &DeviceConfig,
) -> Result<Bytes> {
    let total_len = u32::try_from(xml.len()).map_err(|_| ProtoError::FrameTooLarge(xml.len()))?;
    let frag_size = config.sdk_fragment_size.max(1);

    let mut offset = 0usize;
    loop {
        let end = (offset + frag_size).min(xml.len());
        let frag = SdkFragment {
            total_len,
            offset: offset as u32,
            xml_chunk: xml.slice(offset..end),
        };
        conn.send(OwnedFrame::new(CmdCode::SdkCmd, frag.encode()))
            .await?;
        offset = end;
        if offset >= xml.len() {
            break;
        }
    }

    let mut reasm = SdkReassembler::new();
    loop {
        let reply = read_frame(conn, config.timeout).await?;
        if reply.cmd != CmdCode::SdkReply {
            return Err(Error::UnexpectedReply {
                expected: CmdCode::SdkReply.as_u16(),
                got: reply.cmd.as_u16(),
            });
        }
        if let Some(complete) = reasm.push(SdkFragment::parse(&reply.payload)?)? {
            return Ok(complete);
        }
    }
}

/// Drop-guard implementing the poison-on-cancel rule (`DESIGN.md §4.4`).
///
/// Arm it before the first wire write; disarm it only after a round-trip fully
/// succeeds. If the future is dropped mid-flight — a cancelled caller, or an
/// error unwinding through it — the connection is no longer at a message
/// boundary, so the guard sets `poisoned` and every later command fails fast.
pub(crate) struct PoisonGuard<'a> {
    flag: &'a AtomicBool,
    armed: bool,
}

impl<'a> PoisonGuard<'a> {
    pub(crate) fn new(flag: &'a AtomicBool) -> Self {
        Self { flag, armed: true }
    }

    /// Mark the round-trip complete so dropping the guard is a no-op.
    pub(crate) fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for PoisonGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.flag.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armed_guard_poisons_on_drop() {
        let flag = AtomicBool::new(false);
        drop(PoisonGuard::new(&flag));
        assert!(
            flag.load(Ordering::SeqCst),
            "a dropped, armed guard must poison"
        );
    }

    #[test]
    fn disarmed_guard_leaves_flag_clear() {
        let flag = AtomicBool::new(false);
        PoisonGuard::new(&flag).disarm();
        assert!(
            !flag.load(Ordering::SeqCst),
            "a disarmed guard must not poison"
        );
    }
}
