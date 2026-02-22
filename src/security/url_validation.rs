//! SSRF protection — validates outbound URLs against private/internal IP ranges.

use std::net::IpAddr;

/// Check whether an IP address is private, loopback, link-local, or metadata.
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(v6) => {
            let segs = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || (segs[0] & 0xfe00) == 0xfc00 // unique-local fc00::/7
                || (segs[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || v6.to_ipv4_mapped().is_some_and(|v4| {
                    v4.is_loopback()
                        || v4.is_private()
                        || v4.is_link_local()
                        || v4.is_unspecified()
                        || v4.is_broadcast()
                        || v4.octets() == [169, 254, 169, 254]
                })
        }
    }
}

/// Check whether a hostname string is a private/internal host.
pub fn is_private_host(host: &str) -> bool {
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    if bare == "localhost" {
        return true;
    }
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return is_private_ip(&ip);
    }
    false
}

/// Validate a URL for SSRF safety by resolving DNS and checking all IPs.
pub async fn validate_url_not_ssrf(url_str: &str) -> anyhow::Result<()> {
    let parsed = url::Url::parse(url_str).map_err(|e| anyhow::anyhow!("invalid URL: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL has no host"))?;
    if is_private_host(host) {
        anyhow::bail!("SSRF blocked: host '{host}' resolves to private/internal address");
    }
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addr_str = format!("{host}:{port}");
    if let Ok(addrs) = tokio::net::lookup_host(&addr_str).await {
        for addr in addrs {
            if is_private_ip(&addr.ip()) {
                anyhow::bail!(
                    "SSRF blocked: host '{host}' resolves to private address {}",
                    addr.ip()
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_loopback_v4() {
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn rejects_loopback_v6() {
        let ip: IpAddr = "::1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn rejects_rfc1918_10() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn rejects_rfc1918_172() {
        let ip1: IpAddr = "172.16.0.1".parse().unwrap();
        let ip2: IpAddr = "172.31.255.255".parse().unwrap();
        assert!(is_private_ip(&ip1));
        assert!(is_private_ip(&ip2));
    }

    #[test]
    fn rejects_rfc1918_192() {
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn rejects_link_local() {
        let ip: IpAddr = "169.254.1.1".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn rejects_cloud_metadata() {
        let ip: IpAddr = "169.254.169.254".parse().unwrap();
        assert!(is_private_ip(&ip));
    }

    #[test]
    fn rejects_unique_local_v6() {
        let ip1: IpAddr = "fc00::1".parse().unwrap();
        let ip2: IpAddr = "fd00::1".parse().unwrap();
        assert!(is_private_ip(&ip1));
        assert!(is_private_ip(&ip2));
    }

    #[test]
    fn allows_public_ip() {
        let ip1: IpAddr = "8.8.8.8".parse().unwrap();
        let ip2: IpAddr = "1.1.1.1".parse().unwrap();
        assert!(!is_private_ip(&ip1));
        assert!(!is_private_ip(&ip2));
    }

    #[test]
    fn rejects_localhost_string() {
        assert!(is_private_host("localhost"));
    }

    #[test]
    fn allows_hostname() {
        assert!(!is_private_host("example.com"));
    }
}
