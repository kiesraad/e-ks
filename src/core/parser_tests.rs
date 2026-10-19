//! Property tests for the parsers in `core`, the event hash prefixes and the
//! storage URL: no input panics and an accepted value is canonical (its
//! spelling re-parses to itself).

use proptest::{collection::vec, prelude::*};

use super::{CsbAlertHours, CsbUsername, Locale, Scope, csb_access::parse_ip_nets};
use crate::{
    EventHashPrefix,
    test_support::strategies::{canonical, canonical_display, hostile_string},
    utils::{StorageScheme, format_hash, parse_hash_prefix},
};

/// Hex digits with whitespace, case changes and stray characters mixed in.
fn hex_input(max: usize) -> impl Strategy<Value = String> {
    prop_oneof![
        hostile_string(max),
        vec(
            prop_oneof![
                8 => proptest::char::range('0', '9'),
                4 => proptest::char::range('a', 'f'),
                4 => proptest::char::range('A', 'F'),
                1 => Just(' '),
                1 => Just('\u{a0}'),
                1 => Just('g'),
                1 => Just('٣'),
            ],
            0..=max,
        )
        .prop_map(String::from_iter),
    ]
}

fn is_username_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'@')
}

proptest! {
    #![proptest_config(crate::test_utils::proptest_config(2000))]

    #[test]
    fn csb_username(s in prop_oneof![hostile_string(70), "[a-zA-Z0-9.@_ -]{0,70}"]) {
        let accepted = canonical_display::<CsbUsername>(&s)?.is_some();
        let valid = (1..=64).contains(&s.len()) && s.bytes().all(is_username_byte);
        prop_assert_eq!(accepted, valid);
        prop_assert_eq!(CsbUsername::try_from(s.clone()).is_ok(), valid);
    }

    #[test]
    fn scope(
        s in prop_oneof![
            hostile_string(30),
            "(political_group|central_electoral_committee|imported_by_csb|pre_submitted_to_csb| political_group)",
        ]
    ) {
        canonical(&s, str::parse::<Scope>, |scope| scope.as_str().to_string())?;
    }

    #[test]
    fn locale(s in prop_oneof![hostile_string(4), "(en|nl|EN| nl)"]) {
        canonical_display::<Locale>(&s)?;
    }

    #[test]
    fn event_hash_prefix(s in hex_input(20)) {
        if let Some(prefix) = canonical_display::<EventHashPrefix>(&s)? {
            prop_assert!(s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit()));
            prop_assert_eq!(prefix.to_string(), s.to_ascii_lowercase());
        }
    }

    #[test]
    fn event_hash_prefix_names_its_hash(hash in any::<[u8; 32]>()) {
        let prefix = EventHashPrefix::of(&hash);
        prop_assert_eq!(prefix.to_string().parse::<EventHashPrefix>().ok(), Some(prefix));
        prop_assert!(prefix.matches(&hash));
    }

    #[test]
    fn parse_hash_prefix_accepts_even_hex_of_8_to_64(s in hex_input(70)) {
        let digits: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        let valid = digits.len().is_multiple_of(2)
            && (8..=64).contains(&digits.chars().count())
            && digits.bytes().all(|b| b.is_ascii_hexdigit());
        match parse_hash_prefix(&s) {
            Some(bytes) => {
                prop_assert!(valid, "{:?} accepted", s);
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                prop_assert_eq!(hex, digits.to_ascii_lowercase());
            }
            None => prop_assert!(!valid, "{:?} rejected", s),
        }
    }

    #[test]
    fn format_hash_round_trips(bytes in vec(any::<u8>(), 4..=32), half in any::<bool>()) {
        let shown = format_hash(&bytes, half);
        let expected = if half { &bytes[..bytes.len() / 2] } else { &bytes[..] };
        if expected.len() >= 4 {
            prop_assert_eq!(parse_hash_prefix(&shown), Some(expected.to_vec()));
        }
    }

    #[test]
    fn ip_nets(
        s in prop_oneof![
            hostile_string(30),
            "[ ,]{0,2}([0-9]{1,3}\\.){3}[0-9]{1,3}(/[0-9]{1,2})?([ ]?,[ ]?[0-9a-f:]{2,12}(/[0-9]{1,3})?)?[ ,]{0,2}",
        ]
    ) {
        let spell = |nets: &Vec<ipnet::IpNet>| {
            nets.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")
        };
        if canonical(&s, parse_ip_nets, spell)?.is_some() {
            prop_assert!(s.parse::<super::CsbIpAllowList>().is_ok());
            prop_assert!(s.parse::<super::ProxyProtocolPeers>().is_ok());
        }
    }

    #[test]
    fn csb_alert_hours(
        s in prop_oneof![
            hostile_string(16),
            "[ ]?[0-9 ]{1,3}:[0-9 ]{1,3}[ ]?-[ ]?[0-9]{1,2}:[0-9]{1,2}[ ]?",
        ]
    ) {
        let spell = |h: &CsbAlertHours| {
            format!("{}-{}", h.start.format("%H:%M"), h.end.format("%H:%M"))
        };
        if let Some(hours) = canonical(&s, str::parse::<CsbAlertHours>, spell)? {
            prop_assert_ne!(hours.start, hours.end);
        }
    }

    #[test]
    fn storage_scheme(
        s in prop_oneof![hostile_string(30), "(memory|postgres|postgresql|Postgres|http)://[a-z@:/]{0,12}"]
    ) {
        if let Ok(scheme) = StorageScheme::parse(&s) {
            // URL parsing drops tabs and newlines and trims C0 controls and spaces.
            let url: String = s.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
            let url = url.trim_matches(|c: char| c <= ' ').to_ascii_lowercase();
            let expected = if scheme == StorageScheme::Memory { "memory:" } else { "postgres" };
            prop_assert!(url.starts_with(expected), "{:?}", s);
        }
    }
}

/// The PROXY protocol v2 header: untrusted bytes from the network.
#[cfg(feature = "tls")]
mod proxy_header {
    use std::net::{IpAddr, SocketAddr};

    use proptest::{collection::vec, prelude::*};

    use super::super::proxy_protocol::{PREFIX_LEN, ProxyHeader, parse_v2, payload_len};

    const SIGNATURE: &[u8] = b"\r\n\r\n\0\r\nQUIT\n";

    /// Arbitrary bytes, or a signature and length followed by arbitrary bytes.
    fn header_input() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            vec(any::<u8>(), PREFIX_LEN..80),
            (any::<[u8; 2]>(), vec(any::<u8>(), 0..60)).prop_map(|(kind, payload)| {
                let mut bytes = SIGNATURE.to_vec();
                bytes.extend_from_slice(&kind);
                let len = u16::try_from(payload.len()).expect("short payload");
                bytes.extend_from_slice(&len.to_be_bytes());
                bytes.extend_from_slice(&payload);
                bytes
            }),
        ]
    }

    proptest! {
        #![proptest_config(crate::test_utils::proptest_config(5000))]

        #[test]
        fn parses_without_panicking(bytes in header_input()) {
            let (prefix, payload) = bytes.split_at(PREFIX_LEN);
            let prefix: [u8; PREFIX_LEN] = prefix.try_into().expect("prefix");
            if let Ok(len) = payload_len(&prefix) {
                let payload = &payload[..len.min(payload.len())];
                if let Ok(ProxyHeader::Proxied(addr)) = parse_v2(&prefix, payload) {
                    let port = |at: usize| u16::from_be_bytes([payload[at], payload[at + 1]]);
                    let expected = match addr.ip() {
                        IpAddr::V4(_) => {
                            let ip: [u8; 4] = payload[..4].try_into().expect("v4");
                            SocketAddr::from((ip, port(8)))
                        }
                        IpAddr::V6(_) => {
                            let ip: [u8; 16] = payload[..16].try_into().expect("v6");
                            SocketAddr::from((ip, port(32)))
                        }
                    };
                    prop_assert_eq!(addr, expected);
                }
            }
        }
    }
}
