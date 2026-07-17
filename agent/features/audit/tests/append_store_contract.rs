use std::sync::Arc;

use audit::{file_usage_append_store, AppendLogError, AppendLogNamespace, UsageAppendStorePort};
use sdk::SessionId;
use storage::SafeStorageRoot;

#[tokio::test]
async fn file_append_store_partitions_sessions_and_never_overwrites() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = file_usage_append_store(SafeStorageRoot::open(temp.path()).expect("root"));
    let first = store.stream_for_session(&SessionId::new("session-a"));
    let second = store.stream_for_session(&SessionId::new("session-b"));

    store.append(&first, b"one\n").await.expect("append one");
    store.append(&first, b"two\n").await.expect("append two");
    store
        .append(&second, b"other\n")
        .await
        .expect("append other");
    store.flush(&first).await.expect("flush first");
    store.flush(&second).await.expect("flush second");

    assert_eq!(
        store
            .read(&first)
            .await
            .unwrap()
            .lines()
            .iter()
            .map(|line| line.bytes().to_vec())
            .collect::<Vec<_>>(),
        vec![b"one".to_vec(), b"two".to_vec()]
    );
    assert_eq!(
        store
            .read(&second)
            .await
            .unwrap()
            .lines()
            .iter()
            .map(|line| line.bytes().to_vec())
            .collect::<Vec<_>>(),
        vec![b"other".to_vec()]
    );
    assert_ne!(first, second);
}

#[tokio::test]
async fn file_append_store_lists_sorted_streams_and_flush_survives_reopen() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = SafeStorageRoot::open(temp.path()).expect("root");
    let store = file_usage_append_store(root.clone());
    let b = store.stream_for_session(&SessionId::new("b"));
    let a = store.stream_for_session(&SessionId::new("a"));

    store.append(&b, b"b\n").await.unwrap();
    store.append(&a, b"a\n").await.unwrap();
    store.flush(&a).await.unwrap();
    store.flush(&b).await.unwrap();
    drop(store);

    let reopened = file_usage_append_store(root);
    let streams = reopened
        .list_streams(&AppendLogNamespace::usage())
        .await
        .expect("list");
    assert_eq!(streams, vec![a.clone(), b.clone()]);
    assert_eq!(reopened.read(&a).await.unwrap().lines()[0].bytes(), b"a");
}

#[tokio::test]
async fn file_append_store_rejects_invalid_payload_and_returns_truncated_tail() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = file_usage_append_store(SafeStorageRoot::open(temp.path()).expect("root"));
    let stream = store.stream_for_session(&SessionId::new("session"));

    for invalid in [&b""[..], &b"missing-newline"[..], &b"two\nlines\n"[..]] {
        assert_eq!(
            store.append(&stream, invalid).await,
            Err(AppendLogError::InvalidPayload)
        );
    }

    std::fs::create_dir_all(temp.path().join("usage")).unwrap();
    std::fs::write(
        temp.path()
            .join("usage")
            .join(format!("{}.jsonl", stream.as_str())),
        b"complete\ntruncated",
    )
    .unwrap();

    let reader = store.read(&stream).await.unwrap();
    assert_eq!(reader.lines()[0].bytes(), b"complete");
    assert!(reader.lines()[0].is_terminated());
    assert_eq!(reader.lines()[1].bytes(), b"truncated");
    assert!(!reader.lines()[1].is_terminated());
}

#[tokio::test]
async fn concurrent_appends_preserve_complete_lines() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(file_usage_append_store(
        SafeStorageRoot::open(temp.path()).expect("root"),
    ));
    let stream = store.stream_for_session(&SessionId::new("session"));
    let mut joins = Vec::new();

    for index in 0..32_u8 {
        let store = Arc::clone(&store);
        let stream = stream.clone();
        joins.push(tokio::spawn(async move {
            let line = format!("line-{index:02}\n");
            store.append(&stream, line.as_bytes()).await.unwrap();
        }));
    }
    for join in joins {
        join.await.unwrap();
    }
    store.flush(&stream).await.unwrap();

    let lines = store.read(&stream).await.unwrap().into_lines();
    assert_eq!(lines.len(), 32);
    assert!(lines.iter().all(|line| line.bytes().starts_with(b"line-")
        && line.bytes().len() == 7
        && line.is_terminated()));
}

#[cfg(unix)]
#[tokio::test]
async fn file_append_store_rejects_symlink_stream() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::NamedTempFile::new().expect("outside");
    let store = file_usage_append_store(SafeStorageRoot::open(temp.path()).expect("root"));
    let stream = store.stream_for_session(&SessionId::new("session"));
    std::fs::create_dir_all(temp.path().join("usage")).unwrap();
    symlink(
        outside.path(),
        temp.path()
            .join("usage")
            .join(format!("{}.jsonl", stream.as_str())),
    )
    .unwrap();

    assert_eq!(
        store.append(&stream, b"line\n").await,
        Err(AppendLogError::InvalidStream)
    );
}
