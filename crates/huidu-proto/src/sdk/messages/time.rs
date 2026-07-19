//! `GetTimeInfo` / `SetTimeInfo` — timezone, DST, and clock sync.

use super::SdkMessage;
use crate::error::ProtoError;
use crate::sdk::envelope::SdkReply;
use crate::sdk::method::SdkMethod;
use crate::sdk::xml::{self, XmlWriter};

/// Body-less `GetTimeInfo` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetTime;

impl SdkMessage for GetTime {
    const METHOD: SdkMethod = SdkMethod::GetTimeInfo;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(GetTime)
    }
}

/// The clock body — the `SetTimeInfo` request and `GetTimeInfo` reply share it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimeInfo {
    /// Timezone label, e.g. `(UTC+03:00)Istanbul`.
    pub timezone: String,
    /// Whether daylight-saving is applied.
    pub summer: bool,
    /// Sync mode: `none`, `gps`, `network`, or `auto`.
    pub sync: String,
    /// `YYYY-MM-DD hh:mm:ss`, honored when `sync` is `none`.
    pub time: String,
}

impl SdkMessage for TimeInfo {
    const METHOD: SdkMethod = SdkMethod::SetTimeInfo;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        x.empty("timezone", &[("value", &self.timezone)])?;
        x.empty("summer", &[("enable", xml::bool_str(self.summer))])?;
        x.empty("sync", &[("value", &self.sync)])?;
        x.empty("time", &[("value", &self.time)])?;
        Ok(())
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let mut info = TimeInfo::default();
        xml::elements(&reply.raw, |e| {
            match xml::local_name(e) {
                b"timezone" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.timezone = v;
                    }
                }
                b"summer" => {
                    if let Some(v) = xml::attr(e, "enable")? {
                        info.summer = xml::parse_bool(&v);
                    }
                }
                b"sync" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.sync = v;
                    }
                }
                b"time" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.time = v;
                    }
                }
                _ => {}
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
    fn get_request_round_trips() {
        let bytes = GetTime.encode_request("g").unwrap();
        assert_eq!(GetTime::decode(&bytes).unwrap(), GetTime);
    }

    #[test]
    fn set_request_round_trips() {
        let info = TimeInfo {
            timezone: "(UTC+03:00)Istanbul".into(),
            summer: false,
            sync: "none".into(),
            time: "2026-07-19 14:30:00".into(),
        };
        let bytes = info.encode_request("g").unwrap();
        assert_eq!(TimeInfo::decode(&bytes).unwrap(), info);
    }

    #[test]
    fn get_reply_round_trips() {
        let info = TimeInfo {
            timezone: "(UTC+00:00)UTC".into(),
            summer: true,
            sync: "network".into(),
            time: String::new(),
        };
        let bytes = encode_reply("g", SdkMethod::GetTimeInfo, &SdkResult::success(), |x| {
            info.write_body(x)
        })
        .unwrap();
        assert_eq!(TimeInfo::decode(&bytes).unwrap(), info);
    }
}
