//! Tier-2 file-upload-flow tests against the scripted [`common::MockDevice`]
//! (`DESIGN.md §8`, §10).

mod common;

use std::path::PathBuf;

use common::MockDevice;
use futures::StreamExt;
use huidu::{Device, DeviceConfig, Error};
use md5::{Digest, Md5};

/// The raw MD5 digest of `data`, for asserting the upload hashed the right bytes.
fn md5_of(data: &[u8]) -> [u8; 16] {
    Md5::digest(data).into()
}

/// A temp file that removes itself on drop, so tests leave nothing behind.
struct TempFile(PathBuf);

impl TempFile {
    fn new(tag: &str, bytes: &[u8]) -> Self {
        let path =
            std::env::temp_dir().join(format!("huidu-upload-{}-{tag}.bin", std::process::id()));
        std::fs::write(&path, bytes).expect("write temp file");
        TempFile(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A deterministic byte pattern of `len` bytes.
fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

async fn connect(mock: &MockDevice) -> Device {
    Device::connect(mock.addr(), DeviceConfig::default())
        .await
        .expect("handshake")
}

#[tokio::test]
async fn uploads_whole_file_in_chunks() {
    // 20_000 bytes over the default 8_000-byte chunk size => 3 chunks.
    let data = pattern(20_000);
    let file = TempFile::new("whole", &data);
    let mock = MockDevice::builder().spawn().await;
    let device = connect(&mock).await;

    let stream = device.upload_file(file.path()).await.expect("start upload");
    let progress: Vec<_> = stream.map(|r| r.expect("chunk")).collect().await;

    // Initial (offset 0) progress + one per chunk.
    assert_eq!(progress.first().unwrap().bytes_sent, 0);
    let last = progress.last().unwrap();
    assert!(last.is_complete());
    assert_eq!(last.bytes_sent, 20_000);
    assert_eq!(last.total_bytes, 20_000);

    let cap = mock.upload();
    assert!(cap.started);
    assert_eq!(cap.first_offset, Some(0));
    assert_eq!(
        cap.received, data,
        "device must receive the exact file bytes"
    );
    assert_eq!(cap.declared_size, 20_000);
    assert_eq!(cap.name, file.path().file_name().unwrap().to_string_lossy());
    // The upload must hash the actual file bytes, in both the start and end frame.
    assert_eq!(cap.start_md5, md5_of(&data));
    assert_eq!(cap.end_md5, Some(md5_of(&data)));
    assert!(!device.is_poisoned());

    device.close().await.expect("close");
}

#[tokio::test]
async fn uploads_empty_file() {
    let file = TempFile::new("empty", &[]);
    let mock = MockDevice::builder().spawn().await;
    let device = connect(&mock).await;

    let stream = device.upload_file(file.path()).await.expect("start upload");
    let progress: Vec<_> = stream.map(|r| r.expect("chunk")).collect().await;

    // A single {0,0} progress, no content frames, but start+end still happen.
    assert_eq!(progress.len(), 1);
    assert!(progress[0].is_complete());
    assert_eq!(progress[0].total_bytes, 0);

    let cap = mock.upload();
    assert!(cap.started);
    assert_eq!(
        cap.first_offset, None,
        "an empty file sends no content frames"
    );
    assert!(cap.received.is_empty());
    assert_eq!(cap.start_md5, md5_of(&[]));
    assert_eq!(cap.end_md5, Some(md5_of(&[])));
    assert!(!device.is_poisoned());

    device.close().await.expect("close");
}

#[tokio::test]
async fn resume_at_full_size_sends_no_content() {
    let data = pattern(20_000);
    let file = TempFile::new("resume-complete", &data);
    // Device already holds the whole file; only the end handshake remains.
    let mock = MockDevice::builder()
        .upload_resume_offset(20_000)
        .spawn()
        .await;
    let device = connect(&mock).await;

    let stream = device.upload_file(file.path()).await.expect("start upload");
    let progress: Vec<_> = stream.map(|r| r.expect("chunk")).collect().await;

    assert_eq!(progress.len(), 1, "only the initial (complete) progress");
    assert_eq!(progress[0].bytes_sent, 20_000);
    assert!(progress[0].is_complete());

    let cap = mock.upload();
    assert_eq!(cap.first_offset, None);
    assert!(cap.received.is_empty());
    assert_eq!(cap.end_md5, Some(md5_of(&data)));
    assert!(!device.is_poisoned());

    device.close().await.expect("close");
}

#[tokio::test]
async fn resumes_from_device_offset() {
    let data = pattern(20_000);
    let file = TempFile::new("resume", &data);
    // Device already holds the first 8_000 bytes.
    let mock = MockDevice::builder()
        .upload_resume_offset(8_000)
        .spawn()
        .await;
    let device = connect(&mock).await;

    let stream = device.upload_file(file.path()).await.expect("start upload");
    let progress: Vec<_> = stream.map(|r| r.expect("chunk")).collect().await;

    assert_eq!(progress.first().unwrap().bytes_sent, 8_000);
    assert_eq!(progress.last().unwrap().bytes_sent, 20_000);

    let cap = mock.upload();
    assert_eq!(cap.first_offset, Some(8_000));
    assert_eq!(
        cap.received.as_slice(),
        &data[8_000..],
        "only the bytes past the resume offset are sent"
    );

    device.close().await.expect("close");
}

#[tokio::test]
async fn retries_a_rejected_chunk() {
    let data = pattern(4_000); // single chunk
    let file = TempFile::new("retry", &data);
    // Reject the first two content frames; the third (retry) succeeds.
    let mock = MockDevice::builder()
        .upload_content_fail_times(2)
        .spawn()
        .await;
    let device = connect(&mock).await;

    let stream = device.upload_file(file.path()).await.expect("start upload");
    let progress: Vec<_> = stream.map(|r| r.expect("chunk")).collect().await;

    assert_eq!(progress.last().unwrap().bytes_sent, 4_000);
    let cap = mock.upload();
    assert_eq!(cap.content_attempts, 3, "2 rejections + 1 accepted send");
    assert_eq!(cap.received, data);
    assert!(!device.is_poisoned());

    device.close().await.expect("close");
}

#[tokio::test]
async fn exhausted_retries_error_without_poisoning() {
    let data = pattern(4_000);
    let file = TempFile::new("exhausted", &data);
    // Fail more times than the retry budget (default 3 => 4 attempts total).
    let mock = MockDevice::builder()
        .upload_content_fail_times(99)
        .spawn()
        .await;
    let device = connect(&mock).await;

    let mut stream = Box::pin(device.upload_file(file.path()).await.expect("start upload"));
    // First item is the offset-0 progress; the chunk send then fails.
    let mut err = None;
    while let Some(item) = stream.next().await {
        if let Err(e) = item {
            err = Some(e);
            break;
        }
    }
    drop(stream);

    assert!(
        matches!(
            err,
            Some(Error::Upload {
                stage: "content",
                ..
            })
        ),
        "expected a content-stage upload error, got {err:?}"
    );
    assert_eq!(mock.upload().content_attempts, 4, "1 initial + 3 retries");
    assert!(
        !device.is_poisoned(),
        "a clean device rejection must not poison the connection"
    );

    device.close().await.expect("close");
}

#[tokio::test]
async fn start_rejection_errors_without_poisoning() {
    let data = pattern(1_000);
    let file = TempFile::new("start-reject", &data);
    let mock = MockDevice::builder().upload_start_result(7).spawn().await;
    let device = connect(&mock).await;

    let mut stream = Box::pin(device.upload_file(file.path()).await.expect("start upload"));
    let first = stream.next().await.expect("one item");
    assert!(
        matches!(
            first,
            Err(Error::Upload {
                stage: "start",
                code: 7
            })
        ),
        "expected a start-stage rejection, got {first:?}"
    );
    drop(stream);

    assert!(mock.upload().started);
    assert!(!device.is_poisoned());

    device.close().await.expect("close");
}

#[tokio::test]
async fn dropping_stream_midflight_poisons_connection() {
    let data = pattern(40_000); // several chunks
    let file = TempFile::new("cancel", &data);
    let mock = MockDevice::builder().spawn().await;
    let device = connect(&mock).await;

    let mut stream = Box::pin(device.upload_file(file.path()).await.expect("start upload"));
    // Pull a couple of items so the transfer is mid-flight, then drop it.
    let _ = stream.next().await.expect("first item").expect("ok");
    let _ = stream.next().await.expect("second item").expect("ok");
    assert!(!device.is_poisoned(), "not poisoned mid-transfer");
    drop(stream);

    assert!(
        device.is_poisoned(),
        "dropping the upload stream must poison the connection"
    );

    device.close().await.expect("close");
}
