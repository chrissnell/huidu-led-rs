//! `GetLuminancePloy` / `SetLuminancePloy` — brightness policy.

use super::{parse_int_or, SdkMessage};
use crate::error::ProtoError;
use crate::sdk::envelope::SdkReply;
use crate::sdk::method::SdkMethod;
use crate::sdk::xml::{self, XmlWriter};

/// Body-less `GetLuminancePloy` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetLuminance;

impl SdkMessage for GetLuminance {
    const METHOD: SdkMethod = SdkMethod::GetLuminancePloy;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(GetLuminance)
    }
}

/// How brightness is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LuminanceMode {
    /// Fixed brightness ([`LuminanceInfo::default_value`]).
    #[default]
    Fixed,
    /// Time-of-day schedule ([`LuminanceInfo::items`]).
    Scheduled,
    /// Ambient light sensor, clamped to the sensor range.
    Sensor,
}

impl LuminanceMode {
    fn as_str(self) -> &'static str {
        match self {
            LuminanceMode::Fixed => "default",
            LuminanceMode::Scheduled => "ploys",
            LuminanceMode::Sensor => "sensor",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "ploys" => LuminanceMode::Scheduled,
            "sensor" => LuminanceMode::Sensor,
            _ => LuminanceMode::Fixed,
        }
    }
}

/// One entry in a time-of-day brightness schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuminanceItem {
    pub enabled: bool,
    /// Start of day, `hh:mm:ss`.
    pub start: String,
    /// Brightness percent, 1–100.
    pub percent: u8,
}

impl Default for LuminanceItem {
    fn default() -> Self {
        Self {
            enabled: true,
            start: "00:00:00".into(),
            percent: 100,
        }
    }
}

/// The brightness body — the `SetLuminancePloy` request and `GetLuminancePloy`
/// reply share it. Firmware defaults: 100% fixed, sensor 1–100 over 10s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuminanceInfo {
    pub mode: LuminanceMode,
    pub default_value: u8,
    pub items: Vec<LuminanceItem>,
    pub sensor_min: u8,
    pub sensor_max: u8,
    pub sensor_time: u8,
}

impl Default for LuminanceInfo {
    fn default() -> Self {
        Self {
            mode: LuminanceMode::Fixed,
            default_value: 100,
            items: Vec::new(),
            sensor_min: 1,
            sensor_max: 100,
            sensor_time: 10,
        }
    }
}

impl SdkMessage for LuminanceInfo {
    const METHOD: SdkMethod = SdkMethod::SetLuminancePloy;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        x.empty("mode", &[("value", self.mode.as_str())])?;
        let default_value = self.default_value.to_string();
        x.empty("default", &[("value", &default_value)])?;
        // Firmware wants a `<ploy></ploy>` pair, not a self-closing element.
        x.open("ploy", &[])?;
        for item in &self.items {
            let percent = item.percent.to_string();
            x.empty(
                "item",
                &[
                    ("enable", xml::bool_str(item.enabled)),
                    ("start", &item.start),
                    ("percent", &percent),
                ],
            )?;
        }
        x.close("ploy")?;
        let (min, max, time) = (
            self.sensor_min.to_string(),
            self.sensor_max.to_string(),
            self.sensor_time.to_string(),
        );
        x.empty("sensor", &[("min", &min), ("max", &max), ("time", &time)])?;
        Ok(())
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let mut info = LuminanceInfo {
            items: Vec::new(),
            ..LuminanceInfo::default()
        };
        xml::elements(&reply.raw, |e| {
            match xml::local_name(e) {
                b"mode" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.mode = LuminanceMode::from_str(&v);
                    }
                }
                b"default" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.default_value = parse_int_or(&v, 100);
                    }
                }
                b"item" => {
                    let mut item = LuminanceItem::default();
                    if let Some(v) = xml::attr(e, "enable")? {
                        item.enabled = xml::parse_bool(&v);
                    }
                    if let Some(v) = xml::attr(e, "start")? {
                        item.start = v;
                    }
                    if let Some(v) = xml::attr(e, "percent")? {
                        item.percent = parse_int_or(&v, 100);
                    }
                    info.items.push(item);
                }
                b"sensor" => {
                    if let Some(v) = xml::attr(e, "min")? {
                        info.sensor_min = parse_int_or(&v, 1);
                    }
                    if let Some(v) = xml::attr(e, "max")? {
                        info.sensor_max = parse_int_or(&v, 100);
                    }
                    if let Some(v) = xml::attr(e, "time")? {
                        info.sensor_time = parse_int_or(&v, 10);
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
        let bytes = GetLuminance.encode_request("g").unwrap();
        assert_eq!(GetLuminance::decode(&bytes).unwrap(), GetLuminance);
    }

    #[test]
    fn fixed_brightness_round_trips() {
        let info = LuminanceInfo {
            mode: LuminanceMode::Fixed,
            default_value: 80,
            ..LuminanceInfo::default()
        };
        let bytes = info.encode_request("g").unwrap();
        assert_eq!(LuminanceInfo::decode(&bytes).unwrap(), info);
    }

    #[test]
    fn scheduled_brightness_round_trips() {
        let info = LuminanceInfo {
            mode: LuminanceMode::Scheduled,
            default_value: 100,
            items: vec![
                LuminanceItem {
                    enabled: true,
                    start: "06:00:00".into(),
                    percent: 100,
                },
                LuminanceItem {
                    enabled: true,
                    start: "22:00:00".into(),
                    percent: 20,
                },
            ],
            sensor_min: 5,
            sensor_max: 90,
            sensor_time: 12,
        };
        let bytes = encode_reply(
            "g",
            SdkMethod::GetLuminancePloy,
            &SdkResult::success(),
            |x| info.write_body(x),
        )
        .unwrap();
        assert_eq!(LuminanceInfo::decode(&bytes).unwrap(), info);
    }
}
