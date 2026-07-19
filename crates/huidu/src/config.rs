//! Client configuration (`DESIGN.md §9`). A plain struct with `Default` — no
//! functional-options pattern.

use std::time::Duration;

/// Tunables for a [`crate::Device`]. Construct with `Default` + struct-update:
///
/// ```
/// use huidu::DeviceConfig;
/// use std::time::Duration;
///
/// let config = DeviceConfig {
///     heartbeat: Duration::from_secs(10),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct DeviceConfig {
    /// Per-round-trip deadline for a reply.
    pub timeout: Duration,
    /// Interval between background heartbeat pings.
    pub heartbeat: Duration,
    /// Maximum bytes per file-upload chunk (used by the upload subsystem).
    pub upload_chunk_size: usize,
    /// Maximum XML bytes per SDK fragment before the payload is split.
    pub sdk_fragment_size: usize,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            heartbeat: Duration::from_secs(30),
            upload_chunk_size: 8000,
            sdk_fragment_size: 8000,
        }
    }
}
