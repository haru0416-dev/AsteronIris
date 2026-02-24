use std::net::IpAddr;

/// Returns `true` if the given host string resolves to a private / internal IP.
#[must_use]
pub fn is_private_host(host: &str) -> bool {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_ip(ip);
    }

    matches!(
        host,
        "localhost"
            | "127.0.0.1"
            | "::1"
            | "0.0.0.0"
            | "[::]"
            | "metadata.google.internal"
            | "169.254.169.254"
    )
}

/// Returns `true` when the given IP falls into a private/reserved range.
#[must_use]
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
    }
}

/// Validate that a URL does not point to a private/internal address.
pub fn validate_url_not_ssrf(url_str: &str) -> anyhow::Result<()> {
    let parsed = url::Url::parse(url_str)?;
    if let Some(host) = parsed.host_str()
        && is_private_host(host)
    {
        anyhow::bail!("URL points to private/internal address: {host}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_hosts_detected() {
        assert!(is_private_host("localhost"));
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("::1"));
        assert!(is_private_host("0.0.0.0"));
    }

    #[test]
    fn public_hosts_pass() {
        assert!(!is_private_host("example.com"));
        assert!(!is_private_host("8.8.8.8"));
    }

    #[test]
    fn ssrf_validation_rejects_private() {
        assert!(validate_url_not_ssrf("http://127.0.0.1/secret").is_err());
        assert!(validate_url_not_ssrf("http://localhost/admin").is_err());
    }

    #[test]
    fn ssrf_validation_allows_public() {
        assert!(validate_url_not_ssrf("https://example.com").is_ok());
    }
}
