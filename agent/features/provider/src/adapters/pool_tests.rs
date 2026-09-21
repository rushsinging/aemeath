//! `TransportPool` 行为测试：同 key 复用不可变 transport，key 任一字段
//! 变化即新建 transport，并发 acquire 收敛为单实例。

use std::collections::HashSet;
use std::sync::Arc;

use super::TransportPool;
use crate::adapters::transport::TransportKey;
use crate::domain::capability::ProviderDriverKind;

fn anthropic_key(
    base_url: &str,
    api_key: &str,
    user_agent: &str,
    timeout_secs: u64,
    api_style: Option<&str>,
) -> TransportKey {
    TransportKey {
        driver_kind: ProviderDriverKind::Anthropic,
        api_style: api_style.map(str::to_string),
        base_url: Some(base_url.to_string()),
        api_key: api_key.to_string(),
        user_agent: user_agent.to_string(),
        timeout_secs,
    }
}

fn baseline_key() -> TransportKey {
    anthropic_key(
        "https://api.anthropic.com",
        "test-api-key",
        "aemeath/test",
        300,
        None,
    )
}

#[test]
fn acquire_with_same_key_returns_same_transport_instance() {
    let pool = TransportPool::new();
    let first = pool.acquire(baseline_key());
    let second = pool.acquire(baseline_key());
    assert!(
        Arc::ptr_eq(&first, &second),
        "same key must reuse the pooled transport"
    );
}

#[test]
fn each_key_field_change_builds_distinct_transport() {
    let pool = TransportPool::new();
    let baseline = pool.acquire(baseline_key());

    let variants = vec![
        anthropic_key(
            "https://proxy.example.com",
            "test-api-key",
            "aemeath/test",
            300,
            None,
        ),
        anthropic_key(
            "https://api.anthropic.com",
            "rotated-api-key",
            "aemeath/test",
            300,
            None,
        ),
        anthropic_key(
            "https://api.anthropic.com",
            "test-api-key",
            "aemeath/other-agent",
            300,
            None,
        ),
        anthropic_key(
            "https://api.anthropic.com",
            "test-api-key",
            "aemeath/test",
            120,
            None,
        ),
        anthropic_key(
            "https://api.anthropic.com",
            "test-api-key",
            "aemeath/test",
            300,
            Some("responses"),
        ),
        TransportKey {
            driver_kind: ProviderDriverKind::OpenAI,
            api_style: None,
            base_url: Some("https://api.anthropic.com".to_string()),
            api_key: "test-api-key".to_string(),
            user_agent: "aemeath/test".to_string(),
            timeout_secs: 300,
        },
    ];

    for variant in variants {
        let distinct = pool.acquire(variant);
        assert_ne!(
            distinct.id(),
            baseline.id(),
            "changed transport fact must build a distinct transport"
        );
    }
}

#[test]
fn concurrent_acquire_with_same_key_yields_single_transport() {
    let pool = Arc::new(TransportPool::new());
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let pool = Arc::clone(&pool);
            std::thread::spawn(move || pool.acquire(baseline_key()).id())
        })
        .collect();
    let transports: HashSet<u64> = workers
        .into_iter()
        .map(|worker| worker.join().expect("acquire worker must not panic"))
        .collect();

    assert_eq!(
        transports.len(),
        1,
        "concurrent acquire with one key must converge to a single transport"
    );
}

#[test]
fn distinct_transport_count_reflects_cached_keys_only() {
    let pool = TransportPool::new();
    assert_eq!(pool.distinct_transport_count(), 0);

    pool.acquire(baseline_key());
    assert_eq!(pool.distinct_transport_count(), 1);

    pool.acquire(baseline_key());
    assert_eq!(
        pool.distinct_transport_count(),
        1,
        "same key must not grow the pool"
    );

    pool.acquire(anthropic_key(
        "https://proxy.example.com",
        "test-api-key",
        "aemeath/test",
        300,
        None,
    ));
    assert_eq!(pool.distinct_transport_count(), 2);
}
