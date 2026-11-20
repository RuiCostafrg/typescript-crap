//! Compact function identity: the raw SHA-256 digest of a function's owned tokens.
use serde::{Serialize, Serializer, ser::Error as _};
use std::fmt;
use std::str::FromStr;

/// The SHA-256 identity of a function, stored as raw bytes.
///
/// Displays and serializes as 64 lowercase hexadecimal characters, the same
/// text the report has always used.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionIdentity([u8; 32]);

impl FunctionIdentity {
    /// Wrap a raw SHA-256 digest.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw digest bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Whether `text` is exactly this identity's lowercase hexadecimal form.
    pub(crate) fn matches_hex(&self, text: &str) -> bool {
        let mut hex = [0u8; 64];
        hex::encode_to_slice(self.0, &mut hex).is_ok() && text.as_bytes() == hex
    }
}

impl fmt::Display for FunctionIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut hex = [0u8; 64];
        if hex::encode_to_slice(self.0, &mut hex).is_err() {
            return Err(fmt::Error);
        }
        let hex = std::str::from_utf8(&hex).map_err(|_| fmt::Error)?;
        formatter.write_str(hex)
    }
}

impl fmt::Debug for FunctionIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "FunctionIdentity({self})")
    }
}

/// Returned when a function identity is not exactly 64 hexadecimal characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidFunctionIdentity;

impl fmt::Display for InvalidFunctionIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("function identity must be exactly 64 hexadecimal characters")
    }
}

impl std::error::Error for InvalidFunctionIdentity {}

impl FromStr for FunctionIdentity {
    type Err = InvalidFunctionIdentity;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; 32];
        if value.len() != 64 || hex::decode_to_slice(value, &mut bytes).is_err() {
            return Err(InvalidFunctionIdentity);
        }
        Ok(Self(bytes))
    }
}

impl Serialize for FunctionIdentity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut hex = [0u8; 64];
        if hex::encode_to_slice(self.0, &mut hex).is_err() {
            return Err(S::Error::custom("cannot encode function identity"));
        }
        match std::str::from_utf8(&hex) {
            Ok(hex) => serializer.serialize_str(hex),
            Err(error) => Err(S::Error::custom(error)),
        }
    }
}
