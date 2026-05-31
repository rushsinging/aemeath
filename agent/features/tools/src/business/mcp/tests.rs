#[cfg(test)]
mod mcp_server_config_tests {
    use crate::business::mcp::{
        limit_tool_response, redact_headers, validate_remote_url, McpServerConfig, McpTransportKind,
    };
    use std::collections::HashMap;

    #[test]
    fn test_mcp_server_config_stdio_defaults_to_stdio_transport() {
        let config = McpServerConfig {
            command: Some("/usr/bin/mcp-server".to_string()),
            args: Vec::new(),
            env: HashMap::new(),
            url: None,
            headers: HashMap::new(),
            transport: None,
        };

        assert_eq!(config.transport_kind().unwrap(), McpTransportKind::Stdio);
    }

    #[test]
    fn test_mcp_server_config_url_defaults_to_streamable_http() {
        let config = McpServerConfig {
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            url: Some("https://example.com/mcp".to_string()),
            headers: HashMap::new(),
            transport: None,
        };

        assert_eq!(
            config.transport_kind().unwrap(),
            McpTransportKind::StreamableHttp
        );
    }

    #[test]
    fn test_mcp_server_config_explicit_sse_transport() {
        let config = McpServerConfig {
            command: Some("/usr/bin/mcp-server".to_string()),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some("https://example.com/sse".to_string()),
            headers: HashMap::new(),
            transport: Some(McpTransportKind::Sse),
        };

        assert_eq!(config.transport_kind().unwrap(), McpTransportKind::Sse);
    }

    #[test]
    fn test_validate_remote_url_rejects_public_http() {
        let err = validate_remote_url("http://example.com/mcp").unwrap_err();
        assert!(err.contains("http"));
    }

    #[test]
    fn test_validate_remote_url_allows_localhost_http() {
        validate_remote_url("http://localhost:3000/mcp").unwrap();
        validate_remote_url("http://127.0.0.1:3000/mcp").unwrap();
        validate_remote_url("http://[::1]:3000/mcp").unwrap();
    }

    #[test]
    fn test_redact_headers_hides_sensitive_values() {
        let headers = HashMap::from([
            ("Authorization".to_string(), "Bearer secret".to_string()),
            ("cookie".to_string(), "session=secret".to_string()),
            ("X-Api-Key".to_string(), "secret".to_string()),
            ("Proxy-Authorization".to_string(), "secret".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ]);

        let redacted = redact_headers(&headers);

        assert_eq!(redacted.get("Authorization").unwrap(), "<redacted>");
        assert_eq!(redacted.get("cookie").unwrap(), "<redacted>");
        assert_eq!(redacted.get("X-Api-Key").unwrap(), "<redacted>");
        assert_eq!(redacted.get("Proxy-Authorization").unwrap(), "<redacted>");
        assert_eq!(redacted.get("Accept").unwrap(), "application/json");
    }

    #[test]
    fn test_limit_tool_response_keeps_small_output() {
        assert_eq!(limit_tool_response("hello", 10), "hello");
    }

    #[test]
    fn test_limit_tool_response_truncates_large_output() {
        let limited = limit_tool_response("abcdefghij", 5);

        assert!(limited.starts_with("abcde"));
        assert!(limited.contains("truncated"));
    }

    #[test]
    fn test_limit_tool_response_truncates_at_utf8_boundary() {
        let limited = limit_tool_response("你好世界", 5);

        assert!(limited.starts_with("你"));
        assert!(limited.contains("truncated"));
    }

    #[test]
    fn test_limit_tool_response_zero_limit_returns_notice_without_prefix() {
        assert_eq!(
            limit_tool_response("abc", 0),
            "[Output truncated: original 3 bytes, limit 0 bytes]"
        );
    }

    #[test]
    fn test_limit_tool_response_empty_input_with_zero_limit_returns_empty() {
        assert_eq!(limit_tool_response("", 0), "");
    }
}
