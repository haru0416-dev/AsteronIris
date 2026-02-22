use axum::http::HeaderMap;

pub fn validate_api_key(headers: &HeaderMap, valid_keys: &[String], auth_disabled: bool) -> bool {
    if auth_disabled {
        return true;
    }

    if valid_keys.is_empty() {
        return false;
    }

    let auth_header = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");
    valid_keys.iter().any(|key| key == token)
}

#[cfg(test)]
mod tests {
    use super::validate_api_key;
    use axum::http::HeaderMap;

    #[test]
    fn empty_keys_deny_access() {
        let headers = HeaderMap::new();
        assert!(!validate_api_key(&headers, &[], false));
    }

    #[test]
    fn auth_disabled_explicitly_allows_access() {
        let headers = HeaderMap::new();
        assert!(validate_api_key(&headers, &[], true));
    }

    #[test]
    fn valid_key_returns_true() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer test-key".parse().unwrap());
        let valid_keys = vec!["test-key".to_string()];

        assert!(validate_api_key(&headers, &valid_keys, false));
    }

    #[test]
    fn invalid_key_returns_false() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong-key".parse().unwrap());
        let valid_keys = vec!["test-key".to_string()];

        assert!(!validate_api_key(&headers, &valid_keys, false));
    }

    #[test]
    fn no_authorization_header_returns_false() {
        let headers = HeaderMap::new();
        let valid_keys = vec!["test-key".to_string()];

        assert!(!validate_api_key(&headers, &valid_keys, false));
    }

    #[test]
    fn wrong_prefix_returns_false() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Token test-key".parse().unwrap());
        let valid_keys = vec!["test-key".to_string()];

        assert!(!validate_api_key(&headers, &valid_keys, false));
    }
}
