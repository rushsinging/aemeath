//! daemon_instance HTTP 探活测试。

use super::*;

#[test]
fn endpoint_authority_parses_host_port_and_default_ports() {
    assert_eq!(
        endpoint_authority("http://127.0.0.1:9000/systemone").as_deref(),
        Some("127.0.0.1:9000")
    );
    assert_eq!(
        endpoint_authority("https://scoring.example.com/systemone").as_deref(),
        Some("scoring.example.com:443")
    );
    assert_eq!(
        endpoint_authority("http://scoring.example.com/systemone").as_deref(),
        Some("scoring.example.com:80")
    );
    assert_eq!(
        endpoint_authority("127.0.0.1:9100").as_deref(),
        Some("127.0.0.1:9100"),
        "无 scheme 的 host:port 也可解析"
    );
    assert_eq!(endpoint_authority(""), None);
    assert_eq!(endpoint_authority("http://"), None);
}

#[test]
fn http_connectable_false_for_unlistened_port() {
    assert!(!scoring_http_connectable("http://127.0.0.1:1/systemone"));
}

#[test]
fn http_connectable_true_for_listening_port() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("addr");
    assert!(scoring_http_connectable(&format!(
        "http://{address}/systemone"
    )));
}
