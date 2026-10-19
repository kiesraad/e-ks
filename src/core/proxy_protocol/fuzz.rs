//! Fuzz harness for the PROXY protocol v2 parser (see `crate::fuzzing`).

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use super::{PREFIX_LEN, ProxyHeader, SIGNATURE, parse_v2, payload_len};

/// Reads `data` as the listener reads a connection and checks the outcome
/// against the spec (haproxy `proxy-protocol.txt`, section 2.2).
pub fn proxy_protocol_v2(data: &[u8]) {
    // An odd first byte prepends the signature, so random input gets past it.
    let data: Vec<u8> = match data.split_first() {
        Some((flag, rest)) if flag & 1 == 1 => SIGNATURE.iter().chain(rest).copied().collect(),
        Some((_, rest)) => rest.to_vec(),
        None => return,
    };
    let Some((prefix, rest)) = data.split_first_chunk::<PREFIX_LEN>() else {
        return;
    };

    let len = payload_len(prefix).ok();
    assert_eq!(len, spec_payload_len(prefix), "prefix {prefix:02x?}");
    let Some(len) = len else {
        return;
    };
    // The listener reads exactly `len` bytes; a shorter stream is EOF.
    let Some(payload) = rest.get(..len) else {
        return;
    };
    assert_eq!(
        parse_v2(prefix, payload).ok(),
        spec_header(prefix, payload),
        "prefix {prefix:02x?}, payload {payload:02x?}"
    );
}

/// Version 2 behind the signature: the length is the last two bytes.
fn spec_payload_len(prefix: &[u8; PREFIX_LEN]) -> Option<usize> {
    (prefix[..12] == SIGNATURE && prefix[12] >> 4 == 2)
        .then(|| usize::from(u16::from_be_bytes([prefix[14], prefix[15]])))
}

/// What the header announces, or `None` when it must be refused.
fn spec_header(prefix: &[u8; PREFIX_LEN], payload: &[u8]) -> Option<ProxyHeader> {
    match prefix[12] & 0x0F {
        // LOCAL: the family and addresses are ignored.
        0x0 => return Some(ProxyHeader::Local),
        0x1 => {}
        _ => return None,
    }
    // Only UNSPEC or STREAM can carry the TLS connection.
    if prefix[13] & 0x0F > 1 {
        return None;
    }
    let port = |at: usize| u16::from_be_bytes([payload[at], payload[at + 1]]);
    match prefix[13] >> 4 {
        0x0 => Some(ProxyHeader::Local),
        0x1 if payload.len() >= 12 => {
            let ip: [u8; 4] = payload[..4].try_into().ok()?;
            Some(ProxyHeader::Proxied(SocketAddr::new(
                Ipv4Addr::from(ip).into(),
                port(8),
            )))
        }
        0x2 if payload.len() >= 36 => {
            let ip: [u8; 16] = payload[..16].try_into().ok()?;
            Some(ProxyHeader::Proxied(SocketAddr::new(
                Ipv6Addr::from(ip).into(),
                port(32),
            )))
        }
        _ => None,
    }
}
