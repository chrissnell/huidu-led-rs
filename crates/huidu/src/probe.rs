//! Protocol probing at connect time (`DESIGN.md §6`).
//!
//! HD2020 Gen6 and SDK 2.0 share the same TCP port but are otherwise separate
//! wire protocols. The version handshake (`VersionAsk` → `VersionReply`) runs
//! first on the bare socket and doubles as the probe: [`classify`] reads the
//! reply payload and decides which protocol module owns the connection.

use crate::error::ProtocolKind;
use huidu_proto::hd2020;

/// Classify a device from its version-probe reply payload.
///
/// **Design-derived.** An HD2020 Gen6 controller answers the probe with a
/// payload led by the HD2020 start byte ([`hd2020::START`], `0xA5`); anything
/// else — including the empty payload an SDK 2.0 device sends — is
/// [`ProtocolKind::Sdk2`]. Like the HD2020 framing constants, the exact
/// probe-reply shape has no capture or Go reference in this workspace, so this
/// classifier is a self-consistent placeholder kept in one small function: only
/// its body changes when a real capture lands.
pub(crate) fn classify(payload: &[u8]) -> ProtocolKind {
    match payload.first() {
        Some(&hd2020::START) => ProtocolKind::Hd2020,
        _ => ProtocolKind::Sdk2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_reply_is_sdk2() {
        assert_eq!(classify(&[]), ProtocolKind::Sdk2);
    }

    #[test]
    fn sdk2_marked_reply_is_sdk2() {
        // Any non-0xA5 lead byte stays on the SDK 2.0 path.
        assert_eq!(classify(&[0x00, 0x01]), ProtocolKind::Sdk2);
        assert_eq!(classify(&[0x20, 0x02]), ProtocolKind::Sdk2);
    }

    #[test]
    fn hd2020_start_byte_is_hd2020() {
        assert_eq!(classify(&[hd2020::START]), ProtocolKind::Hd2020);
        assert_eq!(classify(&[hd2020::START, 0x01, 0x02]), ProtocolKind::Hd2020);
    }
}
