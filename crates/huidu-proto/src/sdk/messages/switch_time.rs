//! `GetSwitchTime` / `SetSwitchTime` — scheduled screen on/off rules.

use super::SdkMessage;
use crate::error::ProtoError;
use crate::sdk::envelope::SdkReply;
use crate::sdk::method::SdkMethod;
use crate::sdk::xml::{self, XmlWriter};

/// Body-less `GetSwitchTime` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetSwitchTime;

impl SdkMessage for GetSwitchTime {
    const METHOD: SdkMethod = SdkMethod::GetSwitchTime;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(GetSwitchTime)
    }
}

/// One on/off window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchTimeItem {
    /// `true` opens the screen for the window, `false` closes it.
    pub enabled: bool,
    /// Window start, `hh:mm:ss`.
    pub start: String,
    /// Window end, `hh:mm:ss`.
    pub end: String,
}

impl Default for SwitchTimeItem {
    fn default() -> Self {
        Self {
            enabled: true,
            start: String::new(),
            end: String::new(),
        }
    }
}

/// The schedule body — the `SetSwitchTime` request and `GetSwitchTime` reply
/// share it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchTimeInfo {
    /// Whether the screen is open by default (outside any rule).
    pub open_enabled: bool,
    /// Whether the scheduled rules are active.
    pub ploy_enabled: bool,
    pub items: Vec<SwitchTimeItem>,
}

impl Default for SwitchTimeInfo {
    fn default() -> Self {
        Self {
            open_enabled: true,
            ploy_enabled: false,
            items: Vec::new(),
        }
    }
}

impl SdkMessage for SwitchTimeInfo {
    const METHOD: SdkMethod = SdkMethod::SetSwitchTime;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        x.empty("open", &[("enable", xml::bool_str(self.open_enabled))])?;
        x.open("ploy", &[("enable", xml::bool_str(self.ploy_enabled))])?;
        for item in &self.items {
            x.empty(
                "item",
                &[
                    ("enable", xml::bool_str(item.enabled)),
                    ("start", &item.start),
                    ("end", &item.end),
                ],
            )?;
        }
        x.close("ploy")?;
        Ok(())
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let mut info = SwitchTimeInfo {
            open_enabled: false,
            ..SwitchTimeInfo::default()
        };
        xml::elements(&reply.raw, |e| {
            match xml::local_name(e) {
                b"open" => {
                    if let Some(v) = xml::attr(e, "enable")? {
                        info.open_enabled = xml::parse_bool(&v);
                    }
                }
                b"ploy" => {
                    if let Some(v) = xml::attr(e, "enable")? {
                        info.ploy_enabled = xml::parse_bool(&v);
                    }
                }
                b"item" => {
                    let mut item = SwitchTimeItem::default();
                    if let Some(v) = xml::attr(e, "enable")? {
                        item.enabled = xml::parse_bool(&v);
                    }
                    if let Some(v) = xml::attr(e, "start")? {
                        item.start = v;
                    }
                    if let Some(v) = xml::attr(e, "end")? {
                        item.end = v;
                    }
                    info.items.push(item);
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
        let bytes = GetSwitchTime.encode_request("g").unwrap();
        assert_eq!(GetSwitchTime::decode(&bytes).unwrap(), GetSwitchTime);
    }

    #[test]
    fn set_request_round_trips() {
        let info = SwitchTimeInfo {
            open_enabled: true,
            ploy_enabled: true,
            items: vec![SwitchTimeItem {
                enabled: true,
                start: "08:00:00".into(),
                end: "22:00:00".into(),
            }],
        };
        let bytes = info.encode_request("g").unwrap();
        assert_eq!(SwitchTimeInfo::decode(&bytes).unwrap(), info);
    }

    #[test]
    fn get_reply_round_trips() {
        let info = SwitchTimeInfo {
            open_enabled: false,
            ploy_enabled: true,
            items: vec![
                SwitchTimeItem {
                    enabled: true,
                    start: "06:30:00".into(),
                    end: "12:00:00".into(),
                },
                SwitchTimeItem {
                    enabled: false,
                    start: "12:00:00".into(),
                    end: "13:00:00".into(),
                },
            ],
        };
        let bytes = encode_reply("g", SdkMethod::GetSwitchTime, &SdkResult::success(), |x| {
            info.write_body(x)
        })
        .unwrap();
        assert_eq!(SwitchTimeInfo::decode(&bytes).unwrap(), info);
    }
}
