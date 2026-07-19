//! The `<sdk guid><in|out method result>…</in|out></sdk>` command envelope.
//!
//! [`encode_request`] wraps a body into an `<in>` request; [`encode_reply`]
//! wraps one into an `<out>` reply (used by tests and mock devices).
//! [`decode_reply`] pulls the guid, method, and result back out and keeps the
//! whole document around so a message body can scan it for its own elements.

use crate::error::ProtoError;
use crate::sdk::method::SdkMethod;
use crate::sdk::result::SdkResult;
use crate::sdk::xml::{self, XmlWriter};
use bytes::Bytes;
use std::str::FromStr;

/// The placeholder guid used before the handshake assigns a session guid.
pub const PLACEHOLDER_GUID: &str = "##GUID";

/// Build a request document: `<sdk guid><in method>body</in></sdk>`.
pub fn encode_request(
    guid: &str,
    method: SdkMethod,
    write_body: impl FnOnce(&mut XmlWriter) -> Result<(), ProtoError>,
) -> Result<Bytes, ProtoError> {
    wrap(guid, "in", method, None, write_body)
}

/// Build a reply document: `<sdk guid><out method result>body</out></sdk>`.
pub fn encode_reply(
    guid: &str,
    method: SdkMethod,
    result: &SdkResult,
    write_body: impl FnOnce(&mut XmlWriter) -> Result<(), ProtoError>,
) -> Result<Bytes, ProtoError> {
    wrap(guid, "out", method, Some(result), write_body)
}

fn wrap(
    guid: &str,
    tag: &str,
    method: SdkMethod,
    result: Option<&SdkResult>,
    write_body: impl FnOnce(&mut XmlWriter) -> Result<(), ProtoError>,
) -> Result<Bytes, ProtoError> {
    let mut x = XmlWriter::new();
    x.decl()?;
    x.open("sdk", &[("guid", guid)])?;
    match result {
        Some(r) => x.open(tag, &[("method", method.as_str()), ("result", r.as_str())])?,
        None => x.open(tag, &[("method", method.as_str())])?,
    };
    write_body(&mut x)?;
    x.close(tag)?;
    x.close("sdk")?;
    Ok(Bytes::from(x.into_bytes()))
}

/// A decoded envelope: the header fields plus the full document for body parsing.
#[derive(Debug, Clone)]
pub struct SdkReply {
    /// Session guid from the `<sdk>` element, if present.
    pub guid: Option<String>,
    /// The method named on the `<in>` / `<out>` element.
    pub method: SdkMethod,
    /// The result code, present on `<out>` replies and absent on `<in>` requests.
    pub result: Option<SdkResult>,
    /// The entire document text, so bodies can scan for their own elements.
    pub raw: String,
}

impl SdkReply {
    /// `Ok` if the device reported success (or sent no result at all, as a
    /// request echo does); otherwise a [`ProtoError::SdkError`].
    pub fn check(&self) -> Result<(), ProtoError> {
        match &self.result {
            Some(r) if !r.is_success() => Err(ProtoError::SdkError {
                method: self.method.as_str().to_string(),
                result: r.clone(),
            }),
            _ => Ok(()),
        }
    }
}

/// Parse an envelope document, extracting guid, method, and result. The method
/// element (`in` or `out`) must be present and name a method this crate models.
pub fn decode_reply(bytes: &[u8]) -> Result<SdkReply, ProtoError> {
    let raw = xml::as_str(bytes)?.to_string();
    let mut guid = None;
    let mut method = None;
    let mut result = None;

    xml::elements(&raw, |e| {
        match xml::local_name(e) {
            b"sdk" => guid = xml::attr(e, "guid")?,
            b"in" | b"out" => {
                if let Some(m) = xml::attr(e, "method")? {
                    method = Some(SdkMethod::from_str(&m)?);
                }
                result = xml::attr(e, "result")?.map(|r| SdkResult::from(r.as_str()));
            }
            _ => {}
        }
        Ok(())
    })?;

    let method =
        method.ok_or_else(|| ProtoError::Xml("envelope has no <in>/<out> method".into()))?;
    Ok(SdkReply {
        guid,
        method,
        result,
        raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_guid_and_method() {
        let bytes = encode_request("abc-123", SdkMethod::OpenScreen, |_| Ok(())).unwrap();
        let reply = decode_reply(&bytes).unwrap();
        assert_eq!(reply.guid.as_deref(), Some("abc-123"));
        assert_eq!(reply.method, SdkMethod::OpenScreen);
        assert!(reply.result.is_none());
        reply.check().unwrap();
    }

    #[test]
    fn reply_carries_result_and_checks_success() {
        let bytes = encode_reply("g", SdkMethod::GetDeviceInfo, &SdkResult::success(), |_| {
            Ok(())
        })
        .unwrap();
        let reply = decode_reply(&bytes).unwrap();
        assert_eq!(reply.method, SdkMethod::GetDeviceInfo);
        assert!(reply.result.as_ref().unwrap().is_success());
        reply.check().unwrap();
    }

    #[test]
    fn failure_result_surfaces_as_sdk_error() {
        let bytes = encode_reply(
            "g",
            SdkMethod::SetEth0Info,
            &SdkResult::from("kParseXmlFailed"),
            |_| Ok(()),
        )
        .unwrap();
        let reply = decode_reply(&bytes).unwrap();
        let err = reply.check().unwrap_err();
        assert!(matches!(err, ProtoError::SdkError { .. }));
    }

    #[test]
    fn guid_attribute_is_escaped() {
        // A guid with an ampersand must survive the escape/unescape round-trip.
        let bytes = encode_request("a&b<c>", SdkMethod::CloseScreen, |_| Ok(())).unwrap();
        let reply = decode_reply(&bytes).unwrap();
        assert_eq!(reply.guid.as_deref(), Some("a&b<c>"));
    }
}
