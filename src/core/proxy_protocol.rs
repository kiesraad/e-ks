//! PROXY protocol v2: a proxy announces its client in a binary header sent
//! before the first TLS byte. Only peers in `CSB_PROXY_PROTOCOL_FROM` may
//! send it; anyone else's header is left for the TLS handshake to reject.

#[cfg(feature = "tls")]
use std::{
    fmt,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
};
use std::{net::IpAddr, str::FromStr};

use ipnet::IpNet;

use crate::core::csb_access::{ip_in_nets, parse_ip_nets};

/// Proxies that send a PROXY protocol v2 header (`CSB_PROXY_PROTOCOL_FROM`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyProtocolPeers(Vec<IpNet>);

impl ProxyProtocolPeers {
    /// Whether `ip` is a proxy that must send the header.
    pub fn contains(&self, ip: IpAddr) -> bool {
        ip_in_nets(&self.0, ip)
    }
}

impl FromStr for ProxyProtocolPeers {
    type Err = String;

    /// Parses the comma-separated list; one malformed entry rejects it.
    fn from_str(raw: &str) -> Result<Self, String> {
        parse_ip_nets(raw).map(Self)
    }
}

#[cfg(feature = "tls")]
/// Fixed-size start of a v2 header, ending in the payload length.
pub const PREFIX_LEN: usize = 16;

#[cfg(feature = "tls")]
const SIGNATURE: [u8; 12] = *b"\r\n\r\n\0\r\nQUIT\n";

#[cfg(feature = "tls")]
/// What a v2 header announces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyHeader {
    /// A proxied connection from this client.
    Proxied(SocketAddr),
    /// The proxy's own connection (health check) or unspecified addresses;
    /// the TCP peer stands.
    Local,
}

#[cfg(feature = "tls")]
#[derive(Debug, PartialEq, Eq)]
pub enum ProxyProtocolError {
    MissingSignature,
    VersionOrCommand(u8),
    FamilyOrTransport(u8),
    Truncated(usize),
}

#[cfg(feature = "tls")]
impl fmt::Display for ProxyProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSignature => {
                write!(
                    f,
                    "the connection does not start with a PROXY protocol v2 signature"
                )
            }
            Self::VersionOrCommand(byte) => {
                write!(
                    f,
                    "unsupported PROXY protocol version or command byte {byte:#04x}"
                )
            }
            Self::FamilyOrTransport(byte) => write!(
                f,
                "unsupported PROXY protocol address family or transport byte {byte:#04x}"
            ),
            Self::Truncated(len) => write!(
                f,
                "PROXY protocol header of {len} bytes is too short for its address family"
            ),
        }
    }
}

#[cfg(feature = "tls")]
impl std::error::Error for ProxyProtocolError {}

#[cfg(feature = "tls")]
/// Validates the prefix and returns the payload length.
pub fn payload_len(prefix: &[u8; PREFIX_LEN]) -> Result<usize, ProxyProtocolError> {
    if prefix[..SIGNATURE.len()] != SIGNATURE {
        return Err(ProxyProtocolError::MissingSignature);
    }
    if prefix[12] & 0xF0 != 0x20 {
        return Err(ProxyProtocolError::VersionOrCommand(prefix[12]));
    }
    Ok(usize::from(u16::from_be_bytes([prefix[14], prefix[15]])))
}

#[cfg(feature = "tls")]
/// Parses a header whose prefix passed [`payload_len`]. Trailing TLVs are
/// ignored.
pub fn parse_v2(
    prefix: &[u8; PREFIX_LEN],
    payload: &[u8],
) -> Result<ProxyHeader, ProxyProtocolError> {
    match prefix[12] & 0x0F {
        // LOCAL: the proxy connected on its own behalf.
        0x0 => return Ok(ProxyHeader::Local),
        0x1 => {}
        _ => return Err(ProxyProtocolError::VersionOrCommand(prefix[12])),
    }
    // High nibble: address family; low nibble: transport (0x1 = stream).
    let (family, transport) = (prefix[13] & 0xF0, prefix[13] & 0x0F);
    if transport != 0x1 && transport != 0x0 {
        return Err(ProxyProtocolError::FamilyOrTransport(prefix[13]));
    }
    match family {
        // AF_UNSPEC: addresses not available, the TCP peer stands.
        0x00 => Ok(ProxyHeader::Local),
        0x10 => {
            let addr = payload
                .get(..12)
                .ok_or(ProxyProtocolError::Truncated(payload.len()))?;
            let ip: [u8; 4] = addr[..4].try_into().expect("4 bytes");
            let port = u16::from_be_bytes([addr[8], addr[9]]);
            Ok(ProxyHeader::Proxied(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::from(ip)),
                port,
            )))
        }
        0x20 => {
            let addr = payload
                .get(..36)
                .ok_or(ProxyProtocolError::Truncated(payload.len()))?;
            let ip: [u8; 16] = addr[..16].try_into().expect("16 bytes");
            let port = u16::from_be_bytes([addr[32], addr[33]]);
            Ok(ProxyHeader::Proxied(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::from(ip)),
                port,
            )))
        }
        _ => Err(ProxyProtocolError::FamilyOrTransport(prefix[13])),
    }
}

#[cfg(all(test, feature = "tls"))]
pub(crate) mod test_support {
    use std::net::SocketAddr;

    use super::SIGNATURE;

    /// A v2 header announcing `client` to `server`, followed by `tlvs`.
    pub fn proxy_header(client: SocketAddr, server: SocketAddr, tlvs: &[u8]) -> Vec<u8> {
        let (family, mut addrs) = match (client, server) {
            (SocketAddr::V4(c), SocketAddr::V4(s)) => {
                let mut addrs = Vec::new();
                addrs.extend_from_slice(&c.ip().octets());
                addrs.extend_from_slice(&s.ip().octets());
                (0x11, addrs)
            }
            (SocketAddr::V6(c), SocketAddr::V6(s)) => {
                let mut addrs = Vec::new();
                addrs.extend_from_slice(&c.ip().octets());
                addrs.extend_from_slice(&s.ip().octets());
                (0x21, addrs)
            }
            _ => panic!("mixed address families"),
        };
        addrs.extend_from_slice(&client.port().to_be_bytes());
        addrs.extend_from_slice(&server.port().to_be_bytes());
        addrs.extend_from_slice(tlvs);
        header(0x21, family, &addrs)
    }

    /// A v2 LOCAL header, as sent for health checks.
    pub fn local_header() -> Vec<u8> {
        header(0x20, 0x00, &[])
    }

    pub fn header(version_command: u8, family_transport: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = SIGNATURE.to_vec();
        out.push(version_command);
        out.push(family_transport);
        let len = u16::try_from(payload.len()).expect("payload fits");
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(payload);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peers_accept_addresses_and_ranges() {
        let peers: ProxyProtocolPeers = "172.16.12.9, 10.0.0.0/8".parse().expect("peers");
        assert!(peers.contains("172.16.12.9".parse().unwrap()));
        assert!(peers.contains("10.1.2.3".parse().unwrap()));
        assert!(!peers.contains("172.16.12.10".parse().unwrap()));
        assert!("lb".parse::<ProxyProtocolPeers>().is_err());
        assert!(" ".parse::<ProxyProtocolPeers>().is_err());
    }
}

#[cfg(all(test, feature = "tls"))]
mod header_tests {
    use super::{test_support::*, *};

    fn split(bytes: &[u8]) -> ([u8; PREFIX_LEN], &[u8]) {
        let (prefix, payload) = bytes.split_at(PREFIX_LEN);
        (prefix.try_into().expect("prefix"), payload)
    }

    fn parse(bytes: &[u8]) -> Result<ProxyHeader, ProxyProtocolError> {
        let (prefix, payload) = split(bytes);
        assert_eq!(payload_len(&prefix)?, payload.len());
        parse_v2(&prefix, payload)
    }

    fn addr(s: &str) -> SocketAddr {
        s.parse().expect("socket address")
    }

    #[test]
    fn parses_ipv4_and_ipv6_proxied_connections() {
        let bytes = proxy_header(addr("203.0.113.7:51234"), addr("172.16.12.1:8443"), &[]);
        assert_eq!(
            parse(&bytes),
            Ok(ProxyHeader::Proxied(addr("203.0.113.7:51234")))
        );

        let bytes = proxy_header(addr("[2001:db8::7]:443"), addr("[fd00::1]:8443"), &[]);
        assert_eq!(
            parse(&bytes),
            Ok(ProxyHeader::Proxied(addr("[2001:db8::7]:443")))
        );
    }

    /// TLVs after the addresses are skipped.
    #[test]
    fn ignores_trailing_tlvs() {
        let tlv = [0x20, 0x00, 0x02, 0xAB, 0xCD];
        let bytes = proxy_header(addr("203.0.113.7:51234"), addr("172.16.12.1:8443"), &tlv);
        assert_eq!(
            parse(&bytes),
            Ok(ProxyHeader::Proxied(addr("203.0.113.7:51234")))
        );
    }

    /// LOCAL and unspecified addresses keep the TCP peer.
    #[test]
    fn local_and_unspecified_fall_back_to_the_tcp_peer() {
        assert_eq!(parse(&local_header()), Ok(ProxyHeader::Local));
        assert_eq!(parse(&header(0x21, 0x00, &[])), Ok(ProxyHeader::Local));
        // LOCAL with addresses attached: still ignored.
        let mut bytes = local_header();
        bytes[13] = 0x11;
        bytes[15] = 12;
        bytes.extend_from_slice(&[0; 12]);
        assert_eq!(parse(&bytes), Ok(ProxyHeader::Local));
    }

    #[test]
    fn rejects_other_signatures_versions_commands_and_families() {
        let mut bytes = local_header();
        bytes[0] = b'P';
        assert_eq!(parse(&bytes), Err(ProxyProtocolError::MissingSignature));
        // A v1 header is text, never the v2 signature.
        let (prefix, _) = split(b"PROXY TCP4 1.2.3.4 5.6.7.8 1 2\r\n");
        assert_eq!(
            payload_len(&prefix),
            Err(ProxyProtocolError::MissingSignature)
        );

        assert_eq!(
            parse(&header(0x10, 0x11, &[])),
            Err(ProxyProtocolError::VersionOrCommand(0x10))
        );
        assert_eq!(
            parse(&header(0x22, 0x11, &[])),
            Err(ProxyProtocolError::VersionOrCommand(0x22))
        );
        // AF_UNIX and datagram transports have no place on a TCP listener.
        assert_eq!(
            parse(&header(0x21, 0x31, &[0; 216])),
            Err(ProxyProtocolError::FamilyOrTransport(0x31))
        );
        assert_eq!(
            parse(&header(0x21, 0x12, &[0; 12])),
            Err(ProxyProtocolError::FamilyOrTransport(0x12))
        );
    }

    #[test]
    fn rejects_a_payload_too_short_for_its_family() {
        assert_eq!(
            parse(&header(0x21, 0x11, &[0; 11])),
            Err(ProxyProtocolError::Truncated(11))
        );
        assert_eq!(
            parse(&header(0x21, 0x21, &[0; 35])),
            Err(ProxyProtocolError::Truncated(35))
        );
    }
}
