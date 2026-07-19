//! `AddProgram` / `UpdateProgram` / `DeleteProgram` / `GetProgram` — the method
//! envelopes for pushing and reading screen content.
//!
//! The `Screen → Program → Area → Item` tree and its serialization belong to the
//! `huidu` screen builder (subsystem 6). This layer only carries the screen tree
//! as an opaque, pre-built XML fragment; it does not interpret it. An empty
//! `AddProgram` (no screen) is how the firmware clears all programs.

use super::SdkMessage;
use crate::error::ProtoError;
use crate::sdk::envelope::SdkReply;
use crate::sdk::method::SdkMethod;
use crate::sdk::xml::{self, XmlWriter};

/// Body-less `GetProgram` request. The reply's screen tree is parsed by the
/// screen builder (subsystem 6), so it is left opaque here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetProgram;

impl SdkMessage for GetProgram {
    const METHOD: SdkMethod = SdkMethod::GetProgram;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(GetProgram)
    }
}

/// Body-less `DeleteProgram` request — removes all programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeleteProgram;

impl SdkMessage for DeleteProgram {
    const METHOD: SdkMethod = SdkMethod::DeleteProgram;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(DeleteProgram)
    }
}

/// Carries a pre-built `<screen>` tree for `AddProgram` / `UpdateProgram`.
///
/// The `screen` string must be a well-formed `<screen>…</screen>` fragment; an
/// empty string sends an empty program set. See the module docs for the layering
/// boundary with the screen builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramPush {
    method: SdkMethod,
    /// The serialized screen tree, or empty to clear.
    pub screen: String,
}

impl ProgramPush {
    /// An `AddProgram` push (replaces all programs with `screen`).
    pub fn add(screen: impl Into<String>) -> Self {
        Self {
            method: SdkMethod::AddProgram,
            screen: screen.into(),
        }
    }

    /// An `UpdateProgram` push.
    pub fn update(screen: impl Into<String>) -> Self {
        Self {
            method: SdkMethod::UpdateProgram,
            screen: screen.into(),
        }
    }

    /// The method (`AddProgram` or `UpdateProgram`) this push targets.
    pub fn method(&self) -> SdkMethod {
        self.method
    }

    /// Encode as a request. Unlike the trait default, this honors the instance's
    /// method so one type serves both `AddProgram` and `UpdateProgram`.
    pub fn encode(&self, guid: &str) -> Result<bytes::Bytes, ProtoError> {
        crate::sdk::envelope::encode_request(guid, self.method, |x| self.write_screen(x))
    }

    fn write_screen(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        if !self.screen.is_empty() {
            x.raw(&self.screen)?;
        }
        Ok(())
    }

    /// Decode a request, recovering the method and the embedded screen fragment.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtoError> {
        let reply = crate::sdk::envelope::decode_reply(bytes)?;
        reply.check()?;
        let screen = xml::extract_element(&reply.raw, "screen")?.unwrap_or_default();
        Ok(Self {
            method: reply.method,
            screen,
        })
    }
}

// `SdkMessage` is implemented for uniformity (the getters/clear commands go
// through it); `ProgramPush::encode`/`decode` are the ones callers use so the
// per-instance method is preserved.
impl SdkMessage for ProgramPush {
    const METHOD: SdkMethod = SdkMethod::AddProgram;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        self.write_screen(x)
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let screen = xml::extract_element(&reply.raw, "screen")?.unwrap_or_default();
        Ok(Self {
            method: reply.method,
            screen,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_and_delete_requests_round_trip() {
        let g = GetProgram.encode_request("g").unwrap();
        assert_eq!(GetProgram::decode(&g).unwrap(), GetProgram);
        let d = DeleteProgram.encode_request("g").unwrap();
        assert_eq!(DeleteProgram::decode(&d).unwrap(), DeleteProgram);
    }

    #[test]
    fn add_program_carries_screen_verbatim() {
        let screen = r#"<screen timeStamps="1721400000"><program id="0" guid="p1"/></screen>"#;
        let push = ProgramPush::add(screen);
        let bytes = push.encode("g").unwrap();
        let got = ProgramPush::decode(&bytes).unwrap();
        assert_eq!(got.method(), SdkMethod::AddProgram);
        assert_eq!(got.screen, screen);
    }

    #[test]
    fn update_program_preserves_its_method() {
        let screen = r#"<screen><program id="0"/></screen>"#;
        let push = ProgramPush::update(screen);
        let bytes = push.encode("g").unwrap();
        let got = ProgramPush::decode(&bytes).unwrap();
        assert_eq!(got.method(), SdkMethod::UpdateProgram);
        assert_eq!(got.screen, screen);
    }

    #[test]
    fn empty_add_program_clears() {
        let push = ProgramPush::add("");
        let bytes = push.encode("g").unwrap();
        let got = ProgramPush::decode(&bytes).unwrap();
        assert!(got.screen.is_empty());
    }
}
