//! SDK 2.0 command layer: the XML envelope and the 24 typed method bodies.
//!
//! This sits above the raw frame codec ([`crate::codec`]) and the fragmentation
//! envelope ([`crate::sdk_frame`]): a message body becomes an XML document here,
//! is fragmented by [`crate::sdk_frame`], and framed by [`crate::codec`]. No I/O.

pub mod envelope;
pub mod messages;
pub mod method;
pub mod result;
pub mod xml;

pub use envelope::{decode_reply, encode_reply, encode_request, SdkReply, PLACEHOLDER_GUID};
pub use messages::SdkMessage;
pub use method::SdkMethod;
pub use result::SdkResult;
