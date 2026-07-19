//! `OpenScreen` / `CloseScreen` — light the panel or blank it immediately.
//!
//! Both are body-less commands; the device answers with a result-only `<out>`.

use super::SdkMessage;
use crate::error::ProtoError;
use crate::sdk::envelope::SdkReply;
use crate::sdk::method::SdkMethod;

/// Turn the screen on now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenScreen;

impl SdkMessage for OpenScreen {
    const METHOD: SdkMethod = SdkMethod::OpenScreen;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(OpenScreen)
    }
}

/// Blank the screen now (LEDs off; the panel is not powered down).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CloseScreen;

impl SdkMessage for CloseScreen {
    const METHOD: SdkMethod = SdkMethod::CloseScreen;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(CloseScreen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_close_requests_round_trip() {
        let o = OpenScreen.encode_request("g").unwrap();
        assert_eq!(OpenScreen::decode(&o).unwrap(), OpenScreen);
        let c = CloseScreen.encode_request("g").unwrap();
        assert_eq!(CloseScreen::decode(&c).unwrap(), CloseScreen);
    }
}
