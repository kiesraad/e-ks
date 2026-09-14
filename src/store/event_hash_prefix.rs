//! Unguessable path component binding a download link to its own stream.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer};
use subtle::ConstantTimeEq;

use super::event::EventHash;

/// Leading chain-hash bytes carried in a download URL. 64 bits: far beyond
/// guessing, and short enough to keep the URL readable.
const PREFIX_LEN: usize = 8;

/// The first [`PREFIX_LEN`] bytes of an event's chain hash, lowercase hex.
///
/// Downloads and exports are GET links, which every CSRF check exempts, yet
/// they write audit events. Naming one of the stream's own event hashes is
/// what a cross-site navigation cannot do. Unguessable, not secret: the hash
/// is shown in the audit log and the generated documents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EventHashPrefix([u8; PREFIX_LEN]);

impl EventHashPrefix {
    /// The prefix naming `hash`.
    pub fn of(hash: &EventHash) -> Self {
        let mut prefix = [0u8; PREFIX_LEN];
        prefix.copy_from_slice(&hash[..PREFIX_LEN]);
        Self(prefix)
    }

    /// Whether this prefix names `hash`.
    pub(crate) fn matches(&self, hash: &EventHash) -> bool {
        self.0.ct_eq(&hash[..PREFIX_LEN]).into()
    }

    /// The all-zero prefix: a placeholder shared by every stream, so guessable.
    /// The [`Store`](super::Store) lookups refuse it.
    pub(crate) fn is_genesis(&self) -> bool {
        self.0 == [0u8; PREFIX_LEN]
    }
}

impl fmt::Display for EventHashPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Exactly the spelling [`fmt::Display`] produces, case-insensitively. Any
/// other length is refused, so no shorter prefix can be smuggled in.
impl FromStr for EventHashPrefix {
    type Err = crate::form::ValidationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != PREFIX_LEN * 2 {
            return Err(crate::form::ValidationError::InvalidValue);
        }

        // Hand-rolled rather than `from_str_radix`, which would also accept a
        // sign, giving one prefix several spellings.
        let (pairs, _) = s.as_bytes().as_chunks::<2>();
        let mut prefix = [0u8; PREFIX_LEN];
        for (byte, [high, low]) in prefix.iter_mut().zip(pairs) {
            let (high, low) = (hex_digit(*high), hex_digit(*low));
            *byte = high
                .zip(low)
                .map(|(high, low)| (high << 4) | low)
                .ok_or(crate::form::ValidationError::InvalidValue)?;
        }
        Ok(Self(prefix))
    }
}

/// One hex digit's value.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl<'de> Deserialize<'de> for EventHashPrefix {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::GENESIS_HASH;

    fn hash(first: u8) -> EventHash {
        let mut hash = [0xAAu8; 32];
        hash[0] = first;
        hash
    }

    #[test]
    fn renders_as_lowercase_hex_of_the_leading_bytes() {
        assert_eq!(
            EventHashPrefix::of(&hash(0xF3)).to_string(),
            "f3aaaaaaaaaaaaaa"
        );
    }

    #[test]
    fn round_trips_through_its_own_spelling() {
        let prefix = EventHashPrefix::of(&hash(0x01));

        assert_eq!(prefix.to_string().parse(), Ok(prefix));
        // The rendered form is what a URL carries, so parsing must accept the
        // uppercase spelling a hand-typed URL may use.
        assert_eq!(prefix.to_string().to_uppercase().parse(), Ok(prefix));
    }

    #[test]
    fn matches_only_the_hash_it_names() {
        let prefix = EventHashPrefix::of(&hash(0x01));

        assert!(prefix.matches(&hash(0x01)));
        assert!(!prefix.matches(&hash(0x02)));
        // Differing only past the prefix still matches: the prefix is all the
        // URL carries.
        let mut tail_differs = hash(0x01);
        tail_differs[31] = 0x00;
        assert!(prefix.matches(&tail_differs));
    }

    #[test]
    fn rejects_anything_but_its_exact_spelling() {
        for input in [
            "",
            "f3aaaaaaaaaaaa",          // a byte short
            "f3aaaaaaaaaaaaaaa",       // odd length
            "f3aaaaaaaaaaaaaaaa",      // a byte long
            "f3aaaaaaaaaaaaag",        // non-hex
            "+f+f+f+f+f+f+f+f",        // signs are not hex digits
            "f3 aa aa aa aa aa aa aa", // the spaced form the UI renders
            "f3aaaaaaaaaaaa\u{e9}",    // 16 bytes, but not 16 hex digits
        ] {
            assert_eq!(
                input.parse::<EventHashPrefix>(),
                Err(crate::form::ValidationError::InvalidValue),
                "{input}"
            );
        }
    }

    #[test]
    fn the_genesis_prefix_is_recognised() {
        assert!(EventHashPrefix::of(&GENESIS_HASH).is_genesis());
        assert!(!EventHashPrefix::of(&hash(0x01)).is_genesis());
    }
}
