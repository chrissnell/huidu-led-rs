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

mod config;
mod device;
mod error;
mod probe;
mod transport;

pub use config::DeviceConfig;
pub use device::Device;
pub use error::{Error, ProtocolKind, Result};

/// Hardware and firmware identity cached during the handshake.
pub use huidu_proto::sdk::messages::device_info::DeviceInfo;

/// Text layout for the HD2020 realtime-text send path
/// ([`Device::send_realtime_text`]).
pub use huidu_proto::hd2020::TextLayout;
