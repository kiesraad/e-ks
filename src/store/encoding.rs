//! CBOR encoding of everything this crate persists: event payloads and the
//! on-disk stream frames.
//!
//! CBOR is self-describing — struct fields and enum variants travel as names,
//! not as positions — so adding, removing or reordering either cannot make
//! stored bytes decode as a *different* value. See `docs/code-architecture.md`
//! ("Event encoding") for what that buys and what it still does not cover.

use serde::{Serialize, de::DeserializeOwned};

/// Encode `value` as CBOR.
pub(crate) fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, EncodingError> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).map_err(|e| EncodingError::Encode(e.to_string()))?;
    Ok(out)
}

/// Decode a CBOR value, rejecting input with bytes left over.
///
/// The leftover check is what makes a shrunk type a hard error instead of a
/// silent success: without it, a `T` that stops short of the stored value
/// decodes happily and the remaining fields vanish.
pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, EncodingError> {
    let mut rest = bytes;
    let value =
        ciborium::from_reader(&mut rest).map_err(|e| EncodingError::Decode(e.to_string()))?;

    if !rest.is_empty() {
        return Err(EncodingError::TrailingBytes(rest.len()));
    }

    Ok(value)
}

/// Why a value could not be encoded to, or recovered from, CBOR.
#[derive(Debug)]
pub(crate) enum EncodingError {
    Encode(String),
    Decode(String),
    /// A complete value was decoded, but bytes remained after it.
    TrailingBytes(usize),
}

impl std::fmt::Display for EncodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(err) => write!(f, "CBOR encode failed: {err}"),
            Self::Decode(err) => write!(f, "CBOR decode failed: {err}"),
            Self::TrailingBytes(len) => {
                write!(f, "CBOR decode left {len} trailing byte(s)")
            }
        }
    }
}

impl std::error::Error for EncodingError {}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Original {
        First { label: String },
        Second(u32),
    }

    /// `Original` after a variant was inserted before the others and a field
    /// added: the shape a later build would try to read old bytes with.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Evolved {
        Inserted,
        First {
            label: String,
            #[serde(default)]
            extra: u32,
        },
        Second(u32),
    }

    #[test]
    fn round_trips() {
        let value = Original::First {
            label: "hello".to_string(),
        };
        let bytes = encode(&value).unwrap();
        assert_eq!(decode::<Original>(&bytes).unwrap(), value);
    }

    #[test]
    fn variant_insertion_and_added_field_do_not_shift_meaning() {
        let bytes = encode(&Original::First {
            label: "hello".to_string(),
        })
        .unwrap();

        assert_eq!(
            decode::<Evolved>(&bytes).unwrap(),
            Evolved::First {
                label: "hello".to_string(),
                extra: 0,
            }
        );

        let bytes = encode(&Original::Second(7)).unwrap();
        assert_eq!(decode::<Evolved>(&bytes).unwrap(), Evolved::Second(7));
    }

    #[test]
    fn unknown_variant_is_an_error_not_a_misread() {
        let bytes = encode(&Evolved::Inserted).unwrap();
        assert!(decode::<Original>(&bytes).is_err());
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut bytes = encode(&Original::Second(7)).unwrap();
        bytes.push(0);

        assert!(matches!(
            decode::<Original>(&bytes),
            Err(EncodingError::TrailingBytes(1))
        ));
    }

    #[test]
    fn truncated_input_is_rejected() {
        let bytes = encode(&Original::First {
            label: "hello".to_string(),
        })
        .unwrap();

        assert!(matches!(
            decode::<Original>(&bytes[..bytes.len() - 1]),
            Err(EncodingError::Decode(_))
        ));
    }

    #[test]
    fn byte_fields_encode_as_cbor_byte_strings() {
        #[derive(Serialize, Deserialize)]
        struct Framed {
            #[serde(with = "serde_bytes")]
            payload: Vec<u8>,
        }

        let payload = vec![0xff; 64];
        let bytes = encode(&Framed {
            payload: payload.clone(),
        })
        .unwrap();

        // One byte per payload byte plus a small map/header overhead — not the
        // one-integer-per-byte array a plain `Vec<u8>` would produce.
        assert!(bytes.len() < payload.len() + 20, "got {} bytes", bytes.len());
    }
}
