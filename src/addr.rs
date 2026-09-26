//! Address selection for the docs server.
//!
//! The point of goose-doc is to host goose documentation for *other* machines,
//! so the default bind is every interface. Loopback-only is the opt-in,
//! because a loopback default would make the tool a no-op on a server.

use std::net::{IpAddr, Ipv6Addr};

/// Default bind: all interfaces, so a server is reachable from the network.
pub const DEFAULT_BIND: &str = "0.0.0.0";

/// Addresses that mean "every interface".
pub const ANY_ADDR: &str = "0.0.0.0";
pub const ANY_ADDR_V6: &str = "::";

/// Loopback-only bind, used by `--local-only`.
pub const LOOPBACK: &str = "127.0.0.1";

pub fn is_any(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_unspecified(),
        IpAddr::V6(v6) => v6.is_unspecified(),
    }
}

/// Describe a bind address for humans, so `0.0.0.0` does not look like a bug.
pub fn describe(ip: &IpAddr) -> &'static str {
    if is_any(ip) {
        "all interfaces"
    } else if ip.is_loopback() {
        "this machine only"
    } else {
        "one interface"
    }
}

/// Non-loopback IPv4 addresses, most likely LAN address first.
///
/// Ordering is a heuristic: private ranges (192.168/10/172.16) are almost
/// always what a LAN client should use, while 169.254 link-local and
/// 100.64/10 carrier-grade NAT addresses are usually not.
pub fn local_ips() -> Vec<IpAddr> {
    let mut ips: Vec<IpAddr> = match if_addrs::get_if_addrs() {
        Ok(interfaces) => interfaces
            .into_iter()
            .map(|interface| interface.ip())
            .filter(|ip| !ip.is_loopback())
            .filter(is_usable)
            .collect(),
        Err(_) => Vec::new(),
    };

    ips.sort_by_key(|ip| (rank(ip), ip.to_string()));
    ips.dedup();
    ips
}

/// Candidate addresses for a panel dropdown: everything we can bind to, in the
/// order a human should consider them.
pub fn bind_candidates() -> Vec<String> {
    let mut out = vec![
        DEFAULT_BIND.to_string(),
        LOOPBACK.to_string(),
        "::".to_string(),
    ];
    for ip in local_ips() {
        let text = ip.to_string();
        if !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

/// Best address to put in the URL a user will share.
pub fn preferred_display_ip() -> Option<IpAddr> {
    local_ips().into_iter().next()
}

fn is_usable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_multicast() && !v4.is_broadcast(),
        IpAddr::V6(v6) => !v6.is_multicast() && !is_link_local_v6(v6),
    }
}

/// Rank for ordering: lower sorts first.
fn rank(ip: &IpAddr) -> u8 {
    match ip {
        IpAddr::V4(v4) if v4.is_private() => 0,
        IpAddr::V4(v4) if v4.is_loopback() => 3,
        // 100.64.0.0/10 carrier-grade NAT (Tailscale and friends).
        IpAddr::V4(v4) if v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]) => 1,
        IpAddr::V4(v4) if v4.is_link_local() => 4,
        IpAddr::V4(_) => 2,
        IpAddr::V6(_) => 5,
    }
}

fn is_link_local_v6(ip: &Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(std::net::Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn default_bind_is_all_interfaces() {
        assert_eq!(DEFAULT_BIND, "0.0.0.0");
        assert!(is_any(&DEFAULT_BIND.parse().unwrap()));
    }

    #[test]
    fn describe_explains_the_wildcard() {
        assert_eq!(describe(&DEFAULT_BIND.parse().unwrap()), "all interfaces");
        assert_eq!(describe(&LOOPBACK.parse().unwrap()), "this machine only");
        assert_eq!(describe(&v4(192, 168, 1, 5)), "one interface");
    }

    #[test]
    fn private_addresses_sort_before_public_and_link_local() {
        assert!(rank(&v4(192, 168, 1, 5)) < rank(&v4(8, 8, 8, 8)));
        assert!(rank(&v4(10, 0, 0, 5)) < rank(&v4(8, 8, 8, 8)));
        assert!(rank(&v4(8, 8, 8, 8)) < rank(&v4(169, 254, 1, 1)));
    }

    #[test]
    fn carrier_grade_nat_sorts_after_private() {
        assert!(rank(&v4(192, 168, 1, 5)) < rank(&v4(100, 93, 221, 36)));
        assert!(rank(&v4(100, 93, 221, 36)) < rank(&v4(8, 8, 8, 8)));
    }

    #[test]
    fn link_local_v6_is_detected() {
        let ll: Ipv6Addr = "fe80::1".parse().unwrap();
        assert!(is_link_local_v6(&ll));
        let global: Ipv6Addr = "2001:db8::1".parse().unwrap();
        assert!(!is_link_local_v6(&global));
    }

    #[test]
    fn bind_candidates_start_with_any_then_loopback() {
        let candidates = bind_candidates();
        assert_eq!(candidates[0], "0.0.0.0");
        assert_eq!(candidates[1], "127.0.0.1");
        assert!(candidates.contains(&"::".to_string()));
    }

    #[test]
    fn local_ips_never_include_loopback() {
        for ip in local_ips() {
            assert!(
                !ip.is_loopback(),
                "{ip} should not be offered as a LAN address"
            );
        }
    }
}
