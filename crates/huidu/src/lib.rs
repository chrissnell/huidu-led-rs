//! Async `tokio` client for Huidu LED controllers.
//!
//! Wraps the I/O-free [`huidu_proto`] wire layer with a connection type that
//! performs the 3-phase handshake, runs a background heartbeat, and serializes
//! commands over a single TCP connection. See `DESIGN.md §4`.
//!
//! ```no_run
//! use huidu::{Device, DeviceConfig};
//!
//! # async fn run() -> huidu::Result<()> {
//! let device = Device::connect("192.168.1.50:9527".parse().unwrap(), DeviceConfig::default()).await?;
//! println!("{} — {}x{}", device.info().model, device.info().screen_width, device.info().screen_height);
//! device.close().await?;
//! # Ok(())
//! # }
//! ```

mod commands;
mod config;
mod device;
mod error;
mod transport;

pub use config::DeviceConfig;
pub use device::Device;
pub use error::{Error, ProtocolKind, Result};

/// Typed request/reply bodies the command surface accepts and returns, re-exported
/// so callers never reach into `huidu_proto::sdk::messages`.
pub use huidu_proto::sdk::messages::boot_logo::BootLogoInfo;
/// Hardware and firmware identity cached during the handshake.
pub use huidu_proto::sdk::messages::device_info::DeviceInfo;
pub use huidu_proto::sdk::messages::files::{FileInfo, FileList};
pub use huidu_proto::sdk::messages::luminance::{LuminanceInfo, LuminanceItem, LuminanceMode};
pub use huidu_proto::sdk::messages::network::{EthernetInfo, WifiInfo, WifiMode};
pub use huidu_proto::sdk::messages::server::ServerInfo;
pub use huidu_proto::sdk::messages::switch_time::{SwitchTimeInfo, SwitchTimeItem};
pub use huidu_proto::sdk::messages::time::TimeInfo;
