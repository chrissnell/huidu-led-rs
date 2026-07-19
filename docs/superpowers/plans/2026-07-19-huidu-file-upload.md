# huidu file upload — implementation plan (Subsystem 7)

Reference: `DESIGN.md §8`, §9, §12 item 7.

Chunked, MD5-verified, resume-capable file upload exposed as a progress stream.

## Deliverables

- `huidu-proto::file` — the six file-transfer frame payloads (0x8001–0x8006):
  `FileStartAsk/Reply`, `FileContentAsk/Reply`, `FileEndAsk/Reply`, each with
  `encode()` / `parse()` and round-trip unit tests. Pure bytes, no I/O — it
  belongs beside the SDK envelope in the wire crate.
- `huidu::UploadProgress` + `Device::upload_file(&self, path) ->
  Result<impl Stream<Item = Result<UploadProgress>>>`.
- `DeviceConfig::upload_retries` (chunk retry policy; `upload_chunk_size`
  already exists).
- Tier-2 `MockDevice` upload-flow tests: full upload, resume-on-partial, a
  retried chunk, and a device-rejected start.

## Wire format (provisional)

`DESIGN.md §8` describes the *state machine* but not the byte layout of the
file frames — the same gap Subsystem 1 flagged for the raw `<len>` field. This
plan fixes a clean, self-consistent layout and documents it as **provisional,
to be confirmed against a real capture or the Go reference** before locking any
golden fixtures. All integers little-endian; strings are `u16le` length + UTF-8.

| Frame | Dir | Payload |
|-------|-----|---------|
| `FileStartAsk` 0x8001 | → dev | `name:str`, `type:str`, `size:u64`, `md5:[u8;16]` |
| `FileStartReply` 0x8002 | ← dev | `result:u16`, `resume_offset:u64` |
| `FileContentAsk` 0x8003 | → dev | `offset:u64`, `len:u32`, `data:[u8;len]` |
| `FileContentReply` 0x8004 | ← dev | `result:u16`, `received:u64` |
| `FileEndAsk` 0x8005 | → dev | `md5:[u8;16]` |
| `FileEndReply` 0x8006 | ← dev | `result:u16` |

`result == 0` means OK. A non-zero **content** result is retryable (re-send the
same chunk up to `upload_retries`); a non-zero **start/end** result is fatal.

## State machine (`Device::upload_file`)

1. Open the file, stat its size, stream-hash MD5 from disk (no slurp). Any
   failure here surfaces through the *outer* `Result` before the stream exists.
2. Return a stream that, on first poll: locks the connection, checks the poison
   flag, and arms a `PoisonGuard` for the whole multi-round-trip transaction.
3. `FileStartAsk` → `FileStartReply`: seek the file to the device's
   `resume_offset`, emit the initial `UploadProgress`.
4. Loop: read ≤ `upload_chunk_size` bytes, `FileContentAsk` → `FileContentReply`
   (retrying on a non-zero content result), advance the offset, emit progress.
5. `FileEndAsk` → `FileEndReply`, disarm the guard.

Dropping the stream mid-transfer drops the armed `PoisonGuard` and the lock
guard held inside the generator, so the connection poisons exactly like a
cancelled command (`DESIGN.md §4.4`) — "drop-to-cancel".

### Note on the stream item

`DESIGN.md §8` writes `Stream<Item = UploadProgress>`. A transfer can fail
mid-flight, and the idiomatic way to surface that in a `Stream` is a `Result`
item, so this uses `Item = Result<UploadProgress>`: the outer `Result` is setup
failure, an `Err` item is a mid-transfer failure that ends the stream, and the
final `Ok` item has `bytes_sent == total_bytes`.
