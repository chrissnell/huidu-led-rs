//! Chunked, MD5-verified, resume-capable file upload (`DESIGN.md §8`).
//!
//! [`Device::upload_file`] streams the file's MD5 from disk, then returns a
//! [`Stream`] that runs the `FileStartAsk` → `FileContentAsk`* → `FileEndAsk`
//! state machine, emitting one [`UploadProgress`] per acknowledged chunk.
//!
//! The whole transfer is one transaction: the returned stream holds the
//! connection lock and an armed [`PoisonGuard`] inside its generator, so
//! dropping it mid-flight desyncs and poisons the connection exactly like a
//! cancelled command (`DESIGN.md §4.4`) — that is the "drop-to-cancel".

use std::path::Path;

use async_stream::try_stream;
use bytes::Bytes;
use futures::Stream;
use md5::{Digest, Md5};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::device::Device;
use crate::error::{Error, Result};
use crate::transport::{self, PoisonGuard};
use huidu_proto::{
    CmdCode, FileContentAsk, FileContentReply, FileEndAsk, FileEndReply, FileStartAsk,
    FileStartReply, OwnedFrame,
};

/// Buffer size for the streamed MD5 pass; independent of the upload chunk size.
const HASH_BUF: usize = 64 * 1024;

/// Progress for an in-flight [`Device::upload_file`], emitted once per chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadProgress {
    /// Bytes the device has acknowledged so far (starts at the resume offset).
    pub bytes_sent: u64,
    /// Total file size in bytes.
    pub total_bytes: u64,
}

impl UploadProgress {
    /// Fraction transferred in `0.0..=1.0`; an empty file reports `1.0`.
    pub fn fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            1.0
        } else {
            self.bytes_sent as f64 / self.total_bytes as f64
        }
    }

    /// Whether the final chunk has been acknowledged.
    pub fn is_complete(&self) -> bool {
        self.bytes_sent >= self.total_bytes
    }
}

impl Device {
    /// Upload `path` to the device, returning a progress stream.
    ///
    /// The MD5 is streamed from disk before the stream is returned, so a failure
    /// to open or read the file surfaces through this outer [`Result`]. Each
    /// stream item is the progress after one acknowledged chunk; a mid-transfer
    /// failure is a terminal `Err` item. Dropping the stream cancels the upload
    /// and poisons the connection (`DESIGN.md §4.4`).
    pub async fn upload_file(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<impl Stream<Item = Result<UploadProgress>>> {
        let path = path.as_ref();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file_type = classify(path);

        let mut file = File::open(path).await?;
        let size = file.metadata().await?.len();
        let md5 = stream_md5(&mut file).await?;

        let inner = self.inner.clone();
        Ok(try_stream! {
            let mut frames = inner.frames.lock().await;
            if inner.poisoned.load(std::sync::atomic::Ordering::SeqCst) {
                Err(Error::Poisoned)?;
            }
            // Armed for the whole transaction; a drop or I/O-error early-return
            // leaves it armed and poisons the connection on the way out.
            let mut guard = PoisonGuard::new(&inner.poisoned);
            let config = &inner.config;

            // Step 2 — announce the transfer; the device replies with how much
            // it already holds.
            let start = FileStartAsk { name, file_type, size, md5 };
            let reply = transport::raw_roundtrip(
                &mut frames,
                OwnedFrame::new(CmdCode::FileStartAsk, start.encode()?),
                CmdCode::FileStartReply,
                config.timeout,
            )
            .await?;
            let start_reply = FileStartReply::parse(&reply.payload)?;
            if start_reply.result != 0 {
                // A clean device-level rejection: the round-trip completed, so
                // the stream is still synced — disarm before surfacing it.
                guard.disarm();
                Err(Error::Upload { stage: "start", code: start_reply.result })?;
            }

            let mut offset = start_reply.resume_offset.min(size);
            if start_reply.resume_offset > size {
                tracing::warn!(
                    resume = start_reply.resume_offset, size,
                    "device resume offset exceeds file size; restarting from end",
                );
            }
            file.seek(std::io::SeekFrom::Start(offset)).await?;
            yield UploadProgress { bytes_sent: offset, total_bytes: size };

            // Step 3 — chunk loop. A local disk read/seek error here leaves the
            // device mid-transfer, so the link is logically desynced even though
            // it is byte-synced: the armed guard poisons and the caller
            // reconnects, same as any other mid-transaction failure.
            let mut buf = vec![0u8; config.upload_chunk_size.max(1)];
            while offset < size {
                let n = file.read(&mut buf).await?;
                if n == 0 {
                    break; // truncated underneath us; the end ACK will disagree
                }
                let chunk = Bytes::copy_from_slice(&buf[..n]);
                if let Err(err) =
                    send_chunk(&mut frames, config.timeout, config.upload_retries, offset, chunk)
                        .await
                {
                    // A device NACK arrives on a synced connection; only an I/O
                    // error or timeout desyncs it. Disarm on the former so a
                    // rejected upload doesn't poison an otherwise-usable link.
                    // This assumes a content NACK aborts the device-side
                    // transfer (provisional — confirm against the Go reference).
                    if matches!(err, Error::Upload { .. }) {
                        guard.disarm();
                    }
                    Err(err)?;
                }
                offset += n as u64;
                yield UploadProgress { bytes_sent: offset, total_bytes: size };
            }

            // Step 4 — final verify.
            let reply = transport::raw_roundtrip(
                &mut frames,
                OwnedFrame::new(CmdCode::FileEndAsk, FileEndAsk { md5 }.encode()),
                CmdCode::FileEndReply,
                config.timeout,
            )
            .await?;
            let end_reply = FileEndReply::parse(&reply.payload)?;
            if end_reply.result != 0 {
                guard.disarm();
                Err(Error::Upload { stage: "end", code: end_reply.result })?;
            }

            guard.disarm();
        })
    }
}

/// Send one content chunk, retrying on a non-zero (retryable) device reply up to
/// `retries` times before giving up. A drop or I/O error leaves the caller's
/// poison guard armed; a clean exhausted-retries rejection disarms it there.
async fn send_chunk(
    frames: &mut transport::Conn,
    timeout: std::time::Duration,
    retries: u32,
    offset: u64,
    data: Bytes,
) -> Result<()> {
    let payload = FileContentAsk { offset, data }.encode()?;
    let mut attempt = 0u32;
    loop {
        let reply = transport::raw_roundtrip(
            frames,
            OwnedFrame::new(CmdCode::FileContentAsk, payload.clone()),
            CmdCode::FileContentReply,
            timeout,
        )
        .await?;
        let content_reply = FileContentReply::parse(&reply.payload)?;
        if content_reply.result == 0 {
            // `content_reply.received` (the device's running byte count) is
            // intentionally informational here; we track progress from our own
            // offset. Cross-checking it awaits confirmation of its exact
            // semantics against the Go reference.
            return Ok(());
        }
        if attempt >= retries {
            return Err(Error::Upload {
                stage: "content",
                code: content_reply.result,
            });
        }
        attempt += 1;
        tracing::debug!(
            offset,
            attempt,
            code = content_reply.result,
            "chunk rejected; retrying",
        );
    }
}

/// Hash a file's contents into a raw MD5 digest without slurping it into memory.
async fn stream_md5(file: &mut File) -> Result<[u8; 16]> {
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; HASH_BUF];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    file.seek(std::io::SeekFrom::Start(0)).await?;
    Ok(hasher.finalize().into())
}

/// Map a file extension to the firmware's coarse type tag. Provisional — the
/// real tag set should be confirmed against the Go reference.
fn classify(path: &Path) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "bmp" | "gif" => "image",
        "mp4" | "avi" | "mov" | "mkv" | "flv" => "video",
        _ => "file",
    }
    .to_string()
}
