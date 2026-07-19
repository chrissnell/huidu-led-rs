//! Typed async wrappers over the non-screen SDK 2.0 methods (`DESIGN.md §12`
//! item 5). Each wrapper builds a request from a [`huidu_proto`] message type,
//! round-trips it over the connection, and parses a typed reply.

use crate::device::Device;
use crate::error::Result;
use bytes::Bytes;
use huidu_proto::sdk::messages::boot_logo::{BootLogoInfo, ClearBootLogo, GetBootLogo};
use huidu_proto::sdk::messages::device_info::{DeviceInfo, GetDeviceInfo};
use huidu_proto::sdk::messages::files::{DeleteFiles, FileList, GetFiles};
use huidu_proto::sdk::messages::luminance::{
    GetLuminance, LuminanceInfo, LuminanceItem, LuminanceMode,
};
use huidu_proto::sdk::messages::network::{EthernetInfo, GetEthernet, GetWifi, WifiInfo};
use huidu_proto::sdk::messages::server::{GetServer, ServerInfo};
use huidu_proto::sdk::messages::switch_time::{GetSwitchTime, SwitchTimeInfo};
use huidu_proto::sdk::messages::time::{GetTime, TimeInfo};
use huidu_proto::sdk::{self};
use huidu_proto::{ProtoError, SdkMessage, SdkReplyBody};

impl Device {
    /// Round-trip a request whose reply carries a typed body, decoding it with
    /// the reply type's own `decode` (works for both `SdkMessage` getters and
    /// `SdkReplyBody` reply-only types, which share the decoder signature).
    async fn get_body<T>(
        &self,
        request: Bytes,
        decode: fn(&[u8]) -> std::result::Result<T, ProtoError>,
    ) -> Result<T> {
        let reply = self.send_sdk(request).await?;
        Ok(decode(&reply)?)
    }

    /// Round-trip a setter or action and confirm the device reported success.
    /// The reply body is irrelevant; only its `result` matters.
    async fn exec(&self, request: Bytes) -> Result<()> {
        let reply = self.send_sdk(request).await?;
        sdk::decode_reply(&reply)?.check()?;
        Ok(())
    }

    /// Query fresh `GetDeviceInfo`. Unlike [`Device::info`], which returns the
    /// copy cached at handshake, this re-reads the device.
    pub async fn get_device_info(&self) -> Result<DeviceInfo> {
        let req = GetDeviceInfo.encode_request(self.guid())?;
        self.get_body(req, DeviceInfo::decode).await
    }

    /// Read the device's brightness policy.
    pub async fn get_brightness(&self) -> Result<LuminanceInfo> {
        let req = GetLuminance.encode_request(self.guid())?;
        self.get_body(req, LuminanceInfo::decode).await
    }

    /// Apply a fully-specified brightness policy.
    pub async fn set_brightness(&self, info: &LuminanceInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// Set a fixed brightness `percent` (1–100).
    pub async fn set_brightness_manual(&self, percent: u8) -> Result<()> {
        self.set_brightness(&LuminanceInfo {
            mode: LuminanceMode::Fixed,
            default_value: percent,
            ..Default::default()
        })
        .await
    }

    /// Drive brightness from a time-of-day schedule.
    pub async fn set_brightness_scheduled(&self, items: Vec<LuminanceItem>) -> Result<()> {
        self.set_brightness(&LuminanceInfo {
            mode: LuminanceMode::Scheduled,
            items,
            ..Default::default()
        })
        .await
    }

    /// Drive brightness from the ambient light sensor, clamped to `min..=max`
    /// percent and averaged over `time` seconds.
    pub async fn set_brightness_sensor(&self, min: u8, max: u8, time: u8) -> Result<()> {
        self.set_brightness(&LuminanceInfo {
            mode: LuminanceMode::Sensor,
            sensor_min: min,
            sensor_max: max,
            sensor_time: time,
            ..Default::default()
        })
        .await
    }

    /// Read the Ethernet (`eth0`) configuration.
    pub async fn get_ethernet(&self) -> Result<EthernetInfo> {
        let req = GetEthernet.encode_request(self.guid())?;
        self.get_body(req, EthernetInfo::decode).await
    }

    /// Apply an Ethernet configuration (static, or DHCP when `auto_dhcp`).
    pub async fn set_ethernet(&self, info: &EthernetInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// Read the Wi-Fi configuration.
    pub async fn get_wifi(&self) -> Result<WifiInfo> {
        let req = GetWifi.encode_request(self.guid())?;
        self.get_body(req, WifiInfo::decode).await
    }

    /// Apply a Wi-Fi configuration (access-point or station mode).
    pub async fn set_wifi(&self, info: &WifiInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// Read the device clock, timezone, and DST configuration.
    pub async fn get_time(&self) -> Result<TimeInfo> {
        let req = GetTime.encode_request(self.guid())?;
        self.get_body(req, TimeInfo::decode).await
    }

    /// Set the clock, timezone, DST, and sync mode.
    pub async fn set_time(&self, info: &TimeInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// Read the scheduled screen on/off rules.
    pub async fn get_switch_time(&self) -> Result<SwitchTimeInfo> {
        let req = GetSwitchTime.encode_request(self.guid())?;
        self.get_body(req, SwitchTimeInfo::decode).await
    }

    /// Set the scheduled screen on/off rules.
    pub async fn set_switch_time(&self, info: &SwitchTimeInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// Read the current boot-logo record.
    pub async fn get_boot_logo(&self) -> Result<BootLogoInfo> {
        let req = GetBootLogo.encode_request(self.guid())?;
        self.get_body(req, BootLogoInfo::decode).await
    }

    /// Point the boot logo at an already-uploaded image (by name + md5).
    pub async fn set_boot_logo_name(&self, info: &BootLogoInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// Remove the boot logo, reverting to the firmware default.
    pub async fn clear_boot_logo(&self) -> Result<()> {
        self.exec(ClearBootLogo.encode_request(self.guid())?).await
    }

    /// Read the upstream SDK TCP-server target the device dials into.
    pub async fn get_server(&self) -> Result<ServerInfo> {
        let req = GetServer.encode_request(self.guid())?;
        self.get_body(req, ServerInfo::decode).await
    }

    /// Set the upstream SDK TCP-server host and port.
    pub async fn set_server(&self, info: &ServerInfo) -> Result<()> {
        self.exec(info.encode_request(self.guid())?).await
    }

    /// List the files stored on the device.
    pub async fn list_files(&self) -> Result<FileList> {
        let req = GetFiles.encode_request(self.guid())?;
        self.get_body(req, FileList::decode).await
    }

    /// Delete the named files from the device.
    pub async fn delete_files(
        &self,
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<()> {
        let req = DeleteFiles::new(names).encode_request(self.guid())?;
        self.exec(req).await
    }
}
