//! SDK method names — the `method="…"` attribute on every `<in>` / `<out>`.

use crate::error::ProtoError;

/// Every SDK 2.0 method name this crate knows how to build or read.
///
/// The wire form is the exact CamelCase string the device expects; see
/// [`SdkMethod::as_str`]. `GetIFVersion` (handshake) belongs to the transport
/// subsystem but is listed here so the name set is complete in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkMethod {
    GetIfVersion,
    GetDeviceInfo,
    GetEth0Info,
    SetEth0Info,
    GetWifiInfo,
    SetWifiInfo,
    AddProgram,
    UpdateProgram,
    DeleteProgram,
    GetProgram,
    GetTimeInfo,
    SetTimeInfo,
    GetLuminancePloy,
    SetLuminancePloy,
    GetSwitchTime,
    SetSwitchTime,
    GetFiles,
    DeleteFiles,
    GetBootLogo,
    SetBootLogoName,
    ClearBootLogo,
    GetSdkTcpServer,
    SetSdkTcpServer,
    OpenScreen,
    CloseScreen,
}

impl SdkMethod {
    /// The exact method string the device expects on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            SdkMethod::GetIfVersion => "GetIFVersion",
            SdkMethod::GetDeviceInfo => "GetDeviceInfo",
            SdkMethod::GetEth0Info => "GetEth0Info",
            SdkMethod::SetEth0Info => "SetEth0Info",
            SdkMethod::GetWifiInfo => "GetWifiInfo",
            SdkMethod::SetWifiInfo => "SetWifiInfo",
            SdkMethod::AddProgram => "AddProgram",
            SdkMethod::UpdateProgram => "UpdateProgram",
            SdkMethod::DeleteProgram => "DeleteProgram",
            SdkMethod::GetProgram => "GetProgram",
            SdkMethod::GetTimeInfo => "GetTimeInfo",
            SdkMethod::SetTimeInfo => "SetTimeInfo",
            SdkMethod::GetLuminancePloy => "GetLuminancePloy",
            SdkMethod::SetLuminancePloy => "SetLuminancePloy",
            SdkMethod::GetSwitchTime => "GetSwitchTime",
            SdkMethod::SetSwitchTime => "SetSwitchTime",
            SdkMethod::GetFiles => "GetFiles",
            SdkMethod::DeleteFiles => "DeleteFiles",
            SdkMethod::GetBootLogo => "GetBootLogo",
            SdkMethod::SetBootLogoName => "SetBootLogoName",
            SdkMethod::ClearBootLogo => "ClearBootLogo",
            SdkMethod::GetSdkTcpServer => "GetSDKTcpServer",
            SdkMethod::SetSdkTcpServer => "SetSDKTcpServer",
            SdkMethod::OpenScreen => "OpenScreen",
            SdkMethod::CloseScreen => "CloseScreen",
        }
    }
}

impl std::str::FromStr for SdkMethod {
    type Err = ProtoError;

    /// Parse a method string from a reply, rejecting names we don't model.
    fn from_str(s: &str) -> Result<Self, ProtoError> {
        Ok(match s {
            "GetIFVersion" => SdkMethod::GetIfVersion,
            "GetDeviceInfo" => SdkMethod::GetDeviceInfo,
            "GetEth0Info" => SdkMethod::GetEth0Info,
            "SetEth0Info" => SdkMethod::SetEth0Info,
            "GetWifiInfo" => SdkMethod::GetWifiInfo,
            "SetWifiInfo" => SdkMethod::SetWifiInfo,
            "AddProgram" => SdkMethod::AddProgram,
            "UpdateProgram" => SdkMethod::UpdateProgram,
            "DeleteProgram" => SdkMethod::DeleteProgram,
            "GetProgram" => SdkMethod::GetProgram,
            "GetTimeInfo" => SdkMethod::GetTimeInfo,
            "SetTimeInfo" => SdkMethod::SetTimeInfo,
            "GetLuminancePloy" => SdkMethod::GetLuminancePloy,
            "SetLuminancePloy" => SdkMethod::SetLuminancePloy,
            "GetSwitchTime" => SdkMethod::GetSwitchTime,
            "SetSwitchTime" => SdkMethod::SetSwitchTime,
            "GetFiles" => SdkMethod::GetFiles,
            "DeleteFiles" => SdkMethod::DeleteFiles,
            "GetBootLogo" => SdkMethod::GetBootLogo,
            "SetBootLogoName" => SdkMethod::SetBootLogoName,
            "ClearBootLogo" => SdkMethod::ClearBootLogo,
            "GetSDKTcpServer" => SdkMethod::GetSdkTcpServer,
            "SetSDKTcpServer" => SdkMethod::SetSdkTcpServer,
            "OpenScreen" => SdkMethod::OpenScreen,
            "CloseScreen" => SdkMethod::CloseScreen,
            other => return Err(ProtoError::Xml(format!("unknown SDK method: {other}"))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn every_method_round_trips_through_its_string() {
        let all = [
            SdkMethod::GetIfVersion,
            SdkMethod::GetDeviceInfo,
            SdkMethod::GetEth0Info,
            SdkMethod::SetEth0Info,
            SdkMethod::GetWifiInfo,
            SdkMethod::SetWifiInfo,
            SdkMethod::AddProgram,
            SdkMethod::UpdateProgram,
            SdkMethod::DeleteProgram,
            SdkMethod::GetProgram,
            SdkMethod::GetTimeInfo,
            SdkMethod::SetTimeInfo,
            SdkMethod::GetLuminancePloy,
            SdkMethod::SetLuminancePloy,
            SdkMethod::GetSwitchTime,
            SdkMethod::SetSwitchTime,
            SdkMethod::GetFiles,
            SdkMethod::DeleteFiles,
            SdkMethod::GetBootLogo,
            SdkMethod::SetBootLogoName,
            SdkMethod::ClearBootLogo,
            SdkMethod::GetSdkTcpServer,
            SdkMethod::SetSdkTcpServer,
            SdkMethod::OpenScreen,
            SdkMethod::CloseScreen,
        ];
        for m in all {
            assert_eq!(SdkMethod::from_str(m.as_str()).unwrap(), m);
        }
    }

    #[test]
    fn acronym_casing_is_preserved() {
        assert_eq!(SdkMethod::GetIfVersion.as_str(), "GetIFVersion");
        assert_eq!(SdkMethod::GetSdkTcpServer.as_str(), "GetSDKTcpServer");
    }

    #[test]
    fn unknown_method_errors() {
        assert!(SdkMethod::from_str("NoSuchMethod").is_err());
    }
}
