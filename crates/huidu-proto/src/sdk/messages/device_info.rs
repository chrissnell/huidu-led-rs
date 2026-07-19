//! `GetDeviceInfo` — hardware and firmware identity, screen geometry.

use super::{parse_int_or, SdkMessage};
use crate::error::ProtoError;
use crate::sdk::envelope::SdkReply;
use crate::sdk::method::SdkMethod;
use crate::sdk::xml::{self, XmlWriter};

/// Body-less `GetDeviceInfo` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetDeviceInfo;

impl SdkMessage for GetDeviceInfo {
    const METHOD: SdkMethod = SdkMethod::GetDeviceInfo;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(GetDeviceInfo)
    }
}

/// The `<device>`, `<version>`, and `<screen>` reply to `GetDeviceInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    pub cpu: String,
    pub model: String,
    pub device_id: String,
    pub device_name: String,
    pub fpga_version: String,
    pub app_version: String,
    pub kernel_version: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub screen_rotation: u32,
}

impl SdkMessage for DeviceInfo {
    const METHOD: SdkMethod = SdkMethod::GetDeviceInfo;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        x.empty(
            "device",
            &[
                ("cpu", &self.cpu),
                ("model", &self.model),
                ("id", &self.device_id),
                ("name", &self.device_name),
            ],
        )?;
        x.empty(
            "version",
            &[
                ("fpga", &self.fpga_version),
                ("app", &self.app_version),
                ("kernel", &self.kernel_version),
            ],
        )?;
        let (w, h, r) = (
            self.screen_width.to_string(),
            self.screen_height.to_string(),
            self.screen_rotation.to_string(),
        );
        x.empty("screen", &[("width", &w), ("height", &h), ("rotation", &r)])?;
        Ok(())
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let mut info = DeviceInfo::default();
        xml::elements(&reply.raw, |e| {
            match xml::local_name(e) {
                b"device" => {
                    if let Some(v) = xml::attr(e, "cpu")? {
                        info.cpu = v;
                    }
                    if let Some(v) = xml::attr(e, "model")? {
                        info.model = v;
                    }
                    if let Some(v) = xml::attr(e, "id")? {
                        info.device_id = v;
                    }
                    if let Some(v) = xml::attr(e, "name")? {
                        info.device_name = v;
                    }
                }
                b"version" => {
                    if let Some(v) = xml::attr(e, "fpga")? {
                        info.fpga_version = v;
                    }
                    if let Some(v) = xml::attr(e, "app")? {
                        info.app_version = v;
                    }
                    if let Some(v) = xml::attr(e, "kernel")? {
                        info.kernel_version = v;
                    }
                }
                b"screen" => {
                    if let Some(v) = xml::attr(e, "width")? {
                        info.screen_width = parse_int_or(&v, 0);
                    }
                    if let Some(v) = xml::attr(e, "height")? {
                        info.screen_height = parse_int_or(&v, 0);
                    }
                    if let Some(v) = xml::attr(e, "rotation")? {
                        info.screen_rotation = parse_int_or(&v, 0);
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
    use crate::sdk::result::SdkResult;

    #[test]
    fn get_request_round_trips() {
        let bytes = GetDeviceInfo.encode_request("g").unwrap();
        assert_eq!(GetDeviceInfo::decode(&bytes).unwrap(), GetDeviceInfo);
    }

    #[test]
    fn device_info_reply_round_trips() {
        let info = DeviceInfo {
            cpu: "Freescale.iMax6".into(),
            model: "HD-A30".into(),
            device_id: "HDA30-0001".into(),
            device_name: "Lobby & Sign".into(),
            fpga_version: "1.0".into(),
            app_version: "3.2.1".into(),
            kernel_version: "4.1.15".into(),
            screen_width: 128,
            screen_height: 64,
            screen_rotation: 90,
        };
        let bytes = info.encode_reply("g", &SdkResult::success()).unwrap();
        assert_eq!(DeviceInfo::decode(&bytes).unwrap(), info);
    }
}
