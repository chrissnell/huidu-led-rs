//! The `result` attribute a device stamps on every `<out>` reply.

use std::fmt;

/// The `result="…"` code from an SDK reply, e.g. `kSuccess` or `kParseXmlFailed`.
///
/// Kept as the raw device string rather than a fixed enum: the full `k*` set is
/// firmware-defined and open-ended, so an unknown code must round-trip intact
/// instead of collapsing to a catch-all. Compare against [`SdkResult::SUCCESS`]
/// or call [`SdkResult::is_success`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkResult(String);

impl SdkResult {
    /// The one code that means the command succeeded.
    pub const SUCCESS: &'static str = "kSuccess";

    /// Wrap a raw result string.
    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into())
    }

    /// The success result, ready to stamp on an `<out>` in tests or mocks.
    pub fn success() -> Self {
        Self(Self::SUCCESS.to_string())
    }

    /// The raw result code as written on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the device reported success.
    pub fn is_success(&self) -> bool {
        self.0 == Self::SUCCESS
    }
}

impl fmt::Display for SdkResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for SdkResult {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_is_recognized() {
        assert!(SdkResult::success().is_success());
        assert!(SdkResult::from("kSuccess").is_success());
    }

    #[test]
    fn unknown_code_round_trips_and_is_not_success() {
        let r = SdkResult::from("kParseXmlFailed");
        assert!(!r.is_success());
        assert_eq!(r.as_str(), "kParseXmlFailed");
        assert_eq!(r.to_string(), "kParseXmlFailed");
    }
}
