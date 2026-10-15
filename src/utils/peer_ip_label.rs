use std::net::IpAddr;

/// A peer address for a log field; `unknown` when none was recorded.
pub fn peer_ip_label(ip: Option<IpAddr>) -> String {
    ip.map_or_else(|| "unknown".to_string(), |ip| ip.to_string())
}
