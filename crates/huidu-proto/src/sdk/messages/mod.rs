//! Typed request/response bodies for the 24 SDK 2.0 methods.
//!
//! Two traits keep the request and reply directions honest:
//!
//! - [`SdkMessage`] is for bodies with a valid **request** form — body-less
//!   getters and actions, and setters whose `METHOD` is the set-method they
//!   send. It provides `encode_request` and `decode`.
//! - [`SdkReplyBody`] is for **reply-only** bodies that a device produces but no
//!   client ever sends (a device listing, hardware info). It provides `decode`
//!   and, for tests, `encode_reply` — but deliberately no `encode_request`, so a
//!   reply body can't be mis-encoded as a request under a getter method.
//!
//! Info structs that back both a setter and a getter (ethernet, time, luminance,
//! …) implement [`SdkMessage`] once and serve both directions: `write_body` is
//! the setter's request body and also the getter's reply body, and `parse_body`
//! reads it back either way.

use crate::error::ProtoError;
use crate::sdk::envelope::{self, SdkReply};
use crate::sdk::method::SdkMethod;
use crate::sdk::result::SdkResult;
use crate::sdk::xml::XmlWriter;
use bytes::Bytes;

pub mod boot_logo;
pub mod device_info;
pub mod files;
pub mod luminance;
pub mod network;
pub mod program;
pub mod screen;
pub mod server;
pub mod switch_time;
pub mod time;

/// A round-trippable SDK message body.
///
/// Info structs that back both a setter and a getter (ethernet, time,
/// luminance, …) implement this once and serve both directions: `write_body`
/// is the setter's request body and also the getter's reply body, and
/// `parse_body` reads it back either way.
pub trait SdkMessage: Sized {
    /// The method this body belongs to.
    const METHOD: SdkMethod;

    /// Write the body's elements. Body-less commands leave this as the default.
    fn write_body(&self, _x: &mut XmlWriter) -> Result<(), ProtoError> {
        Ok(())
    }

    /// Reconstruct the body from a decoded envelope by scanning its elements.
    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError>;

    /// Encode this body as a request: `<sdk guid><in method>body</in></sdk>`.
    fn encode_request(&self, guid: &str) -> Result<Bytes, ProtoError> {
        envelope::encode_request(guid, Self::METHOD, |x| self.write_body(x))
    }

    /// Encode this body as a reply — for tests and mock devices.
    fn encode_reply(&self, guid: &str, result: &SdkResult) -> Result<Bytes, ProtoError> {
        envelope::encode_reply(guid, Self::METHOD, result, |x| self.write_body(x))
    }

    /// Decode a document into this body, first surfacing any device error.
    fn decode(bytes: &[u8]) -> Result<Self, ProtoError> {
        let reply = envelope::decode_reply(bytes)?;
        reply.check()?;
        Self::parse_body(&reply)
    }
}

/// A reply-only body: the device produces it, no client sends it. Unlike
/// [`SdkMessage`] there is no `encode_request` — the method is a getter, which
/// carries no request body, so encoding one would be malformed.
pub trait SdkReplyBody: Sized {
    /// The getter method this body answers.
    const METHOD: SdkMethod;

    /// Write the body's elements — used to build reply fixtures in tests.
    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError>;

    /// Reconstruct the body from a decoded reply by scanning its elements.
    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError>;

    /// Encode this body as a reply — for tests and mock devices.
    fn encode_reply(&self, guid: &str, result: &SdkResult) -> Result<Bytes, ProtoError> {
        envelope::encode_reply(guid, Self::METHOD, result, |x| self.write_body(x))
    }

    /// Decode a reply document into this body, first surfacing any device error.
    fn decode(bytes: &[u8]) -> Result<Self, ProtoError> {
        let reply = envelope::decode_reply(bytes)?;
        reply.check()?;
        Self::parse_body(&reply)
    }
}

/// Parse `s` as an integer, keeping `default` on failure — matches the firmware's
/// lenient `sscanf` behavior so a malformed attribute never fails the whole reply.
pub(crate) fn parse_int_or<T: std::str::FromStr>(s: &str, default: T) -> T {
    s.trim().parse().unwrap_or(default)
}
