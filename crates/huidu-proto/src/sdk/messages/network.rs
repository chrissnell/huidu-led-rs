//! `GetEth0Info` / `SetEth0Info` and `GetWifiInfo` / `SetWifiInfo`.

use super::SdkMessage;
use crate::error::ProtoError;
use crate::sdk::envelope::SdkReply;
use crate::sdk::method::SdkMethod;
use crate::sdk::xml::{self, XmlWriter};

// ─── Ethernet ───────────────────────────────────────────────────────────────

/// Body-less `GetEth0Info` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetEthernet;

impl SdkMessage for GetEthernet {
    const METHOD: SdkMethod = SdkMethod::GetEth0Info;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(GetEthernet)
    }
}

/// The `<eth>` body — the `SetEth0Info` request and `GetEth0Info` reply share it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EthernetInfo {
    pub enabled: bool,
    pub auto_dhcp: bool,
    pub ip: String,
    pub netmask: String,
    pub gateway: String,
    pub dns: String,
}

impl SdkMessage for EthernetInfo {
    const METHOD: SdkMethod = SdkMethod::SetEth0Info;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        x.open("eth", &[("valid", "true")])?;
        x.empty("enable", &[("value", xml::bool_str(self.enabled))])?;
        x.empty("dhcp", &[("auto", xml::bool_str(self.auto_dhcp))])?;
        x.empty(
            "address",
            &[
                ("ip", &self.ip),
                ("netmask", &self.netmask),
                ("gateway", &self.gateway),
                ("dns", &self.dns),
            ],
        )?;
        x.close("eth")?;
        Ok(())
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let mut info = EthernetInfo::default();
        xml::elements(&reply.raw, |e| {
            match xml::local_name(e) {
                b"enable" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.enabled = xml::parse_bool(&v);
                    }
                }
                b"dhcp" => {
                    if let Some(v) = xml::attr(e, "auto")? {
                        info.auto_dhcp = xml::parse_bool(&v);
                    }
                }
                b"address" => {
                    if let Some(v) = xml::attr(e, "ip")? {
                        info.ip = v;
                    }
                    if let Some(v) = xml::attr(e, "netmask")? {
                        info.netmask = v;
                    }
                    if let Some(v) = xml::attr(e, "gateway")? {
                        info.gateway = v;
                    }
                    if let Some(v) = xml::attr(e, "dns")? {
                        info.dns = v;
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(info)
    }
}

// ─── Wi-Fi ────────────────────────────────────────────────────────────────────

/// Body-less `GetWifiInfo` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetWifi;

impl SdkMessage for GetWifi {
    const METHOD: SdkMethod = SdkMethod::GetWifiInfo;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(GetWifi)
    }
}

/// Whether the Wi-Fi module runs its own access point or joins one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WifiMode {
    /// The device hosts an access point.
    #[default]
    Ap,
    /// The device joins an existing network.
    Station,
}

impl WifiMode {
    fn as_str(self) -> &'static str {
        match self {
            WifiMode::Ap => "ap",
            WifiMode::Station => "station",
        }
    }

    fn from_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("station") {
            WifiMode::Station
        } else {
            WifiMode::Ap
        }
    }
}

/// The `<wifi>` body — the `SetWifiInfo` request and `GetWifiInfo` reply share it.
///
/// Modeled around the access-point fields the firmware reliably reports; full
/// station-mode addressing is layered on by the command surface (subsystem 5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WifiInfo {
    pub has_wifi: bool,
    pub enabled: bool,
    pub mode: WifiMode,
    pub ssid: String,
    pub password: String,
    pub channel: String,
    pub encryption: String,
}

impl SdkMessage for WifiInfo {
    const METHOD: SdkMethod = SdkMethod::SetWifiInfo;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        x.empty("wifi", &[("valid", xml::bool_str(self.has_wifi))])?;
        x.empty("enable", &[("value", xml::bool_str(self.enabled))])?;
        x.empty("mode", &[("value", self.mode.as_str())])?;
        x.open("ap", &[])?;
        x.empty("ssid", &[("value", &self.ssid)])?;
        x.empty("passwd", &[("value", &self.password)])?;
        x.empty("channel", &[("value", &self.channel)])?;
        x.empty("encryption", &[("value", &self.encryption)])?;
        x.close("ap")?;
        Ok(())
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let mut info = WifiInfo::default();
        xml::elements(&reply.raw, |e| {
            match xml::local_name(e) {
                b"wifi" => {
                    if let Some(v) = xml::attr(e, "valid")? {
                        info.has_wifi = xml::parse_bool(&v);
                    }
                }
                b"enable" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.enabled = xml::parse_bool(&v);
                    }
                }
                b"mode" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.mode = WifiMode::from_str(&v);
                    }
                }
                b"ssid" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.ssid = v;
                    }
                }
                b"passwd" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.password = v;
                    }
                }
                b"channel" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.channel = v;
                    }
                }
                b"encryption" => {
                    if let Some(v) = xml::attr(e, "value")? {
                        info.encryption = v;
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
    fn ethernet_get_request_round_trips() {
        let bytes = GetEthernet.encode_request("g").unwrap();
        assert_eq!(GetEthernet::decode(&bytes).unwrap(), GetEthernet);
    }

    #[test]
    fn ethernet_set_request_round_trips() {
        let info = EthernetInfo {
            enabled: true,
            auto_dhcp: false,
            ip: "192.168.6.1".into(),
            netmask: "255.255.255.0".into(),
            gateway: "192.168.6.254".into(),
            dns: "8.8.8.8".into(),
        };
        let bytes = info.encode_request("g").unwrap();
        assert_eq!(EthernetInfo::decode(&bytes).unwrap(), info);
    }

    #[test]
    fn ethernet_get_reply_round_trips() {
        let info = EthernetInfo {
            enabled: true,
            auto_dhcp: true,
            ip: "10.0.0.5".into(),
            netmask: "255.255.255.0".into(),
            gateway: "10.0.0.1".into(),
            dns: "1.1.1.1".into(),
        };
        let bytes = encode_reply("g", SdkMethod::GetEth0Info, &SdkResult::success(), |x| {
            info.write_body(x)
        })
        .unwrap();
        assert_eq!(EthernetInfo::decode(&bytes).unwrap(), info);
    }

    #[test]
    fn wifi_get_request_round_trips() {
        let bytes = GetWifi.encode_request("g").unwrap();
        assert_eq!(GetWifi::decode(&bytes).unwrap(), GetWifi);
    }

    #[test]
    fn wifi_set_request_round_trips() {
        let info = WifiInfo {
            has_wifi: true,
            enabled: true,
            mode: WifiMode::Station,
            ssid: "office-net".into(),
            password: "s3cr3t".into(),
            channel: "6".into(),
            encryption: "WPA-PSK".into(),
        };
        let bytes = info.encode_request("g").unwrap();
        assert_eq!(WifiInfo::decode(&bytes).unwrap(), info);
    }
}
