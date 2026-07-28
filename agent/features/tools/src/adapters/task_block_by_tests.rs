use super::*;

fn test_ctx() -> ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::path::PathBuf::from("."))
        .build()
}

fn task_spec(subject: &str) -> task::TaskCreateSpec {
    task::TaskCreateSpec::try_new(
        subject.into(),
        String::new(),
        None,
        task::TaskPriority::Normal,
    )
    .unwrap()
}

fn setup() -> (Arc<task::TaskStore>, Vec<task::TaskId>) {
    let store = Arc::new(task::TaskStore::new());
    store
        .create_batch(task::BatchCreateSpec::try_new("batch".into()).unwrap(), 1)
        .unwrap();
    let ids = ["目标", "前置一", "前置二"]
        .into_iter()
        .enumerate()
        .map(|(index, subject)| {
            store
                .create_task(task_spec(subject), index as u64 + 2)
                .unwrap()
                .value
                .id()
        })
        .collect();
    (store, ids)
}

#[tokio::test]
async fn task_block_by_replaces_complete_dependency_list_and_clears_it() {
    let (store, ids) = setup();
    let access: Arc<dyn task::TaskAccess> = store.clone();
    let tool = TaskBlockByTool { access };

    let replaced = tool
        .call(
            serde_json::json!({"id": "1", "block_by_ids": ["2", "3"]}),
            &test_ctx(),
        )
        .await;
    assert!(!replaced.is_error, "{}", replaced.text);
    assert_eq!(store.get(ids[0]).unwrap().blocked_by(), &[ids[1], ids[2]]);
    assert_eq!(
        replaced.data.unwrap().blocked_by_ids,
        vec!["2".to_string(), "3".to_string()]
    );

    let cleared = tool
        .call(
            serde_json::json!({"id": "1", "block_by_ids": []}),
            &test_ctx(),
        )
        .await;
    assert!(!cleared.is_error, "{}", cleared.text);
    assert!(store.get(ids[0]).unwrap().blocked_by().is_empty());
}

#[tokio::test]
async fn task_block_by_rejects_duplicate_and_unknown_ids_without_mutation() {
    let (store, ids) = setup();
    let access: Arc<dyn task::TaskAccess> = store.clone();
    let tool = TaskBlockByTool { access };

    for input in [
        serde_json::json!({"id": "1", "block_by_ids": ["2", "2"]}),
        serde_json::json!({"id": "1", "block_by_ids": ["99"]}),
        serde_json::json!({"id": "1", "block_by_ids": ["1"]}),
    ] {
        let revision_before = store.revision();
        let result = tool.call(input, &test_ctx()).await;
        assert!(result.is_error);
        assert_eq!(store.revision(), revision_before);
        assert!(store.get(ids[0]).unwrap().blocked_by().is_empty());
    }
}
