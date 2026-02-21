use super::domain::{extract_host, host_matches_allowlist, is_private_host, normalize_domains};
use super::tool_impl::BrowserTool;
use crate::intelligence::tools::traits::Tool;
use crate::security::SecurityPolicy;
use std::sync::Arc;

#[test]
fn normalize_domains_works() {
    let domains = vec![
        "  Example.COM  ".into(),
        "docs.example.com".into(),
        String::new(),
    ];
    let normalized = normalize_domains(domains);
    assert_eq!(normalized, vec!["example.com", "docs.example.com"]);
}

#[test]
fn extract_host_works() {
    assert_eq!(
        extract_host("https://example.com/path").unwrap(),
        "example.com"
    );
    assert_eq!(
        extract_host("https://Sub.Example.COM:8080/").unwrap(),
        "sub.example.com"
    );
}

#[test]
fn extract_host_handles_ipv6() {
    // IPv6 with brackets (required for URLs with ports)
    assert_eq!(extract_host("https://[::1]/path").unwrap(), "[::1]");
    // IPv6 with brackets and port
    assert_eq!(
        extract_host("https://[2001:db8::1]:8080/path").unwrap(),
        "[2001:db8::1]"
    );
    // IPv6 with brackets, trailing slash
    assert_eq!(extract_host("https://[fe80::1]/").unwrap(), "[fe80::1]");
}

#[test]
fn is_private_host_detects_local() {
    assert!(is_private_host("localhost"));
    assert!(is_private_host("127.0.0.1"));
    assert!(is_private_host("192.168.1.1"));
    assert!(is_private_host("10.0.0.1"));
    assert!(!is_private_host("example.com"));
    assert!(!is_private_host("google.com"));
}

#[test]
fn is_private_host_catches_ipv6() {
    assert!(is_private_host("::1"));
    assert!(is_private_host("[::1]"));
    assert!(is_private_host("0.0.0.0"));
}

#[test]
fn is_private_host_catches_mapped_ipv4() {
    // IPv4-mapped IPv6 addresses
    assert!(is_private_host("::ffff:127.0.0.1"));
    assert!(is_private_host("::ffff:10.0.0.1"));
    assert!(is_private_host("::ffff:192.168.1.1"));
}

#[test]
fn is_private_host_catches_ipv6_private_ranges() {
    // Unique-local (fc00::/7)
    assert!(is_private_host("fd00::1"));
    assert!(is_private_host("fc00::1"));
    // Link-local (fe80::/10)
    assert!(is_private_host("fe80::1"));
    // Public IPv6 should pass
    assert!(!is_private_host("2001:db8::1"));
}

#[test]
fn validate_url_blocks_ipv6_ssrf() {
    let security = Arc::new(SecurityPolicy::default());
    let tool = BrowserTool::new(security, vec!["*".into()], None);
    assert!(tool.validate_url("https://[::1]/").is_err());
    assert!(tool.validate_url("https://[::ffff:127.0.0.1]/").is_err());
    assert!(
        tool.validate_url("https://[::ffff:10.0.0.1]:8080/")
            .is_err()
    );
}

#[test]
fn host_matches_allowlist_exact() {
    let allowed = vec!["example.com".into()];
    assert!(host_matches_allowlist("example.com", &allowed));
    assert!(host_matches_allowlist("sub.example.com", &allowed));
    assert!(!host_matches_allowlist("notexample.com", &allowed));
}

#[test]
fn host_matches_allowlist_wildcard() {
    let allowed = vec!["*.example.com".into()];
    assert!(host_matches_allowlist("sub.example.com", &allowed));
    assert!(host_matches_allowlist("example.com", &allowed));
    assert!(!host_matches_allowlist("other.com", &allowed));
}

#[test]
fn host_matches_allowlist_star() {
    let allowed = vec!["*".into()];
    assert!(host_matches_allowlist("anything.com", &allowed));
    assert!(host_matches_allowlist("example.org", &allowed));
}

#[test]
fn browser_tool_name() {
    let security = Arc::new(SecurityPolicy::default());
    let tool = BrowserTool::new(security, vec!["example.com".into()], None);
    assert_eq!(tool.name(), "browser");
}

#[test]
fn browser_tool_validates_url() {
    let security = Arc::new(SecurityPolicy::default());
    let tool = BrowserTool::new(security, vec!["example.com".into()], None);

    // Valid
    assert!(tool.validate_url("https://example.com").is_ok());
    assert!(tool.validate_url("https://sub.example.com/path").is_ok());

    // Invalid - not in allowlist
    assert!(tool.validate_url("https://other.com").is_err());

    // Invalid - private host
    assert!(tool.validate_url("https://localhost").is_err());
    assert!(tool.validate_url("https://127.0.0.1").is_err());

    // Invalid - not https
    assert!(tool.validate_url("ftp://example.com").is_err());

    // File URLs allowed
    assert!(tool.validate_url("file:///tmp/test.html").is_ok());
}

#[test]
fn browser_tool_empty_allowlist_blocks() {
    let security = Arc::new(SecurityPolicy::default());
    let tool = BrowserTool::new(security, vec![], None);
    assert!(tool.validate_url("https://example.com").is_err());
}
