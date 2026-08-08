use crate::error::HostIntegrationError;

pub(crate) fn validate_local_endpoint(
    endpoint: &str,
    integration: &str,
) -> Result<(), HostIntegrationError> {
    if is_local_proxy_endpoint(endpoint) && !endpoint.ends_with('/') {
        return Ok(());
    }

    Err(HostIntegrationError::InvalidIntegration(format!(
        "{integration} 接入只允许使用有效的本地代理地址，收到 {endpoint}"
    )))
}

pub(crate) fn is_local_proxy_endpoint(endpoint: &str) -> bool {
    let endpoint = endpoint.strip_suffix('/').unwrap_or(endpoint);
    let Some(port) = endpoint.strip_prefix("http://127.0.0.1:") else {
        return false;
    };

    !port.is_empty()
        && port.chars().all(|character| character.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|value| value > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_proxy_endpoint_requires_loopback_http_and_valid_port() {
        assert!(is_local_proxy_endpoint("http://127.0.0.1:1"));
        assert!(is_local_proxy_endpoint("http://127.0.0.1:65535/"));
        assert!(!is_local_proxy_endpoint("https://127.0.0.1:51234"));
        assert!(!is_local_proxy_endpoint("http://localhost:51234"));
        assert!(!is_local_proxy_endpoint("http://127.0.0.1:0"));
        assert!(!is_local_proxy_endpoint("http://127.0.0.1:65536"));
    }

    #[test]
    fn managed_endpoint_uses_canonical_form_without_trailing_slash() {
        assert!(validate_local_endpoint("http://127.0.0.1:51234", "CLI").is_ok());
        assert!(validate_local_endpoint("http://127.0.0.1:51234/", "CLI").is_err());
    }
}
