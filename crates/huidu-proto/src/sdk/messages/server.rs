//! `GetSDKTcpServer` / `SetSDKTcpServer` — the upstream TCP server the device
//! dials into.

use super::{parse_int_or, SdkMessage};
use crate::error::ProtoError;
use crate::sdk::envelope::SdkReply;
use crate::sdk::method::SdkMethod;
use crate::sdk::xml::{self, XmlWriter};

/// The device's default TCP port.
pub const DEFAULT_PORT: u16 = 10001;

/// Body-less `GetSDKTcpServer` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetServer;

impl SdkMessage for GetServer {
    const METHOD: SdkMethod = SdkMethod::GetSdkTcpServer;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(GetServer)
    }
}

/// The `<server host port>` body — the `SetSDKTcpServer` request and the
/// `GetSDKTcpServer` reply share this shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    pub host: String,
    pub port: u16,
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: DEFAULT_PORT,
        }
    }
}

impl SdkMessage for ServerInfo {
    const METHOD: SdkMethod = SdkMethod::SetSdkTcpServer;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        let port = self.port.to_string();
        x.empty("server", &[("host", &self.host), ("port", &port)])?;
        Ok(())
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let mut info = ServerInfo::default();
        xml::elements(&reply.raw, |e| {
            if xml::local_name(e) == b"server" {
                if let Some(v) = xml::attr(e, "host")? {
                    info.host = v;
                }
                if let Some(v) = xml::attr(e, "port")? {
                    info.port = parse_int_or(&v, DEFAULT_PORT);
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
    use crate::sdk::result::SdkResult;

    #[test]
    fn get_request_round_trips() {
        let bytes = GetServer.encode_request("g").unwrap();
        assert_eq!(GetServer::decode(&bytes).unwrap(), GetServer);
    }

    #[test]
    fn set_request_round_trips() {
        let info = ServerInfo {
            host: "srv.example.com".into(),
            port: 6000,
        };
        let bytes = info.encode_request("g").unwrap();
        assert_eq!(ServerInfo::decode(&bytes).unwrap(), info);
    }

    #[test]
    fn get_reply_round_trips() {
        let info = ServerInfo {
            host: "10.0.0.9".into(),
            port: 10001,
        };
        let bytes = crate::sdk::envelope::encode_reply(
            "g",
            SdkMethod::GetSdkTcpServer,
            &SdkResult::success(),
            |x| info.write_body(x),
        )
        .unwrap();
        assert_eq!(ServerInfo::decode(&bytes).unwrap(), info);
    }
}
