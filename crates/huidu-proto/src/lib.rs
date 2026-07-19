//! I/O-free wire protocol layer for Huidu LED controllers.
//!
//! Turns raw TCP bytes into typed frames and back, and reassembles fragmented
//! SDK XML payloads. No networking lives here — see the `huidu` crate for that.

pub mod codec;
pub mod error;
pub mod frame;
pub mod sdk;
pub mod sdk_frame;

pub use codec::HuiduCodec;
pub use error::ProtoError;
pub use frame::{CmdCode, OwnedFrame};
pub use sdk::{SdkMessage, SdkMethod, SdkResult};
pub use sdk_frame::{SdkFragment, SdkReassembler};
