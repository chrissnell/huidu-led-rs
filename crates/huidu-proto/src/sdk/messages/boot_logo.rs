//! `GetBootLogo` / `SetBootLogoName` / `ClearBootLogo` — the splash image.

use super::SdkMessage;
use crate::error::ProtoError;
use crate::sdk::envelope::SdkReply;
use crate::sdk::method::SdkMethod;
use crate::sdk::xml::{self, XmlWriter};

/// Body-less `GetBootLogo` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetBootLogo;

impl SdkMessage for GetBootLogo {
    const METHOD: SdkMethod = SdkMethod::GetBootLogo;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(GetBootLogo)
    }
}

/// Body-less `ClearBootLogo` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClearBootLogo;

impl SdkMessage for ClearBootLogo {
    const METHOD: SdkMethod = SdkMethod::ClearBootLogo;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(ClearBootLogo)
    }
}

/// The `<logo>` body — the `SetBootLogoName` request and `GetBootLogo` reply
/// share it. Upload the image first, then name it here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootLogoInfo {
    pub exists: bool,
    pub name: String,
    pub md5: String,
}

impl SdkMessage for BootLogoInfo {
    const METHOD: SdkMethod = SdkMethod::SetBootLogoName;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        x.empty(
            "logo",
            &[
                ("exist", xml::bool_str(self.exists)),
                ("name", &self.name),
                ("md5", &self.md5),
            ],
        )?;
        Ok(())
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let mut info = BootLogoInfo::default();
        xml::elements(&reply.raw, |e| {
            if xml::local_name(e) == b"logo" {
                if let Some(v) = xml::attr(e, "exist")? {
                    info.exists = xml::parse_bool(&v);
                }
                if let Some(v) = xml::attr(e, "name")? {
                    info.name = v;
                }
                if let Some(v) = xml::attr(e, "md5")? {
                    info.md5 = v;
                }
            }
            Ok(())
        })?;
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdk::envelope::encode_reply;
    use crate::sdk::result::SdkResult;

    #[test]
    fn body_less_requests_round_trip() {
        let g = GetBootLogo.encode_request("g").unwrap();
        assert_eq!(GetBootLogo::decode(&g).unwrap(), GetBootLogo);
        let c = ClearBootLogo.encode_request("g").unwrap();
        assert_eq!(ClearBootLogo::decode(&c).unwrap(), ClearBootLogo);
    }

    #[test]
    fn set_request_round_trips() {
        let info = BootLogoInfo {
            exists: true,
            name: "splash.png".into(),
            md5: "deadbeef".into(),
        };
        let bytes = info.encode_request("g").unwrap();
        assert_eq!(BootLogoInfo::decode(&bytes).unwrap(), info);
    }

    #[test]
    fn get_reply_round_trips() {
        let info = BootLogoInfo {
            exists: false,
            name: String::new(),
            md5: String::new(),
        };
        let bytes = encode_reply("g", SdkMethod::GetBootLogo, &SdkResult::success(), |x| {
            info.write_body(x)
        })
        .unwrap();
        assert_eq!(BootLogoInfo::decode(&bytes).unwrap(), info);
    }
}
