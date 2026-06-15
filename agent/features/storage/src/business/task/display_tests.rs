use super::*;
use crate::business::task::{Batch, Task, TaskPriority};

async fn setup_store_with_batches() -> TaskStore {
    TaskStore::new()
}

async fn add_task(store: &TaskStore, id: &str, batch: u64, status: TaskStatus) {
    let task = Task {
        id: id.to_string(),
        subject: format!("task-{id}"),
        description: String::new(),
        status,
        batch,
        active_form: None,
        owner: None,
        blocked_by: vec![],
        blocks: vec![],
        priority: TaskPriority::Normal,
        progress: 0,
        progress_message: None,
        created_at: 0,
        updated_at: 0,
        session_id: None,
        tags: vec![],
    };
    store.tasks.lock().await.insert(task.id.clone(), task);
}

// --- resolve_display_id ---

#[tokio::test]
async fn test_resolve_display_id_local_number_in_current_batch() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1, 0));
    // Batch 1 has tasks with global ids 8, 9, 10
    add_task(&store, "8", 1, TaskStatus::Pending).await;
    add_task(&store, "9", 1, TaskStatus::InProgress).await;
    add_task(&store, "10", 1, TaskStatus::Pending).await;

    // Display number 2 should resolve to global id "9"
    let result = store.resolve_display_id("2").await;
    assert_eq!(result, Some("9".to_string()));
}

#[tokio::test]
async fn test_resolve_display_id_fallback_to_global_id() {
    let store = setup_store_with_batches().await;
    // No batch, but task with id "42" exists
    add_task(&store, "42", 0, TaskStatus::Pending).await;

    let result = store.resolve_display_id("42").await;
    assert_eq!(result, Some("42".to_string()));
}

#[tokio::test]
async fn test_resolve_display_id_not_found() {
    let store = setup_store_with_batches().await;
    let result = store.resolve_display_id("999").await;
    assert!(result.is_none());
}

// --- get_display_number ---

#[tokio::test]
async fn test_get_display_number_within_batch() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1, 0));
    add_task(&store, "8", 1, TaskStatus::Pending).await;
    add_task(&store, "9", 1, TaskStatus::InProgress).await;
    add_task(&store, "10", 1, TaskStatus::Pending).await;

    // Global id "8" → display number 1
    assert_eq!(store.get_display_number("8").await, Some(1));
    // Global id "9" → display number 2
    assert_eq!(store.get_display_number("9").await, Some(2));
    // Global id "10" → display number 3
    assert_eq!(store.get_display_number("10").await, Some(3));
}

#[tokio::test]
async fn test_get_display_number_task_not_found() {
    let store = setup_store_with_batches().await;
    assert_eq!(store.get_display_number("999").await, None);
}

#[tokio::test]
async fn test_get_display_number_excludes_deleted() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1, 0));
    add_task(&store, "1", 1, TaskStatus::Pending).await;
    add_task(&store, "2", 1, TaskStatus::Deleted).await;
    add_task(&store, "3", 1, TaskStatus::InProgress).await;

    // "1" is display 1, "3" is display 2 (deleted "2" excluded)
    assert_eq!(store.get_display_number("1").await, Some(1));
    assert_eq!(store.get_display_number("3").await, Some(2));
    // Deleted task "2" has no display number
    assert_eq!(store.get_display_number("2").await, None);
}

// --- resolve_display_ids / to_display_ids ---

#[tokio::test]
async fn test_resolve_display_ids_batch() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1, 0));
    add_task(&store, "5", 1, TaskStatus::Pending).await;
    add_task(&store, "6", 1, TaskStatus::Pending).await;

    let result = store
        .resolve_display_ids(&["1".to_string(), "2".to_string()])
        .await;
    assert_eq!(result, vec!["5", "6"]);
}

#[tokio::test]
async fn test_to_display_ids_batch() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1, 0));
    add_task(&store, "5", 1, TaskStatus::Pending).await;
    add_task(&store, "6", 1, TaskStatus::Pending).await;

    let result = store
        .to_display_ids(&["5".to_string(), "6".to_string()])
        .await;
    assert_eq!(result, vec!["1", "2"]);
}

// --- display_batch_id ---

#[tokio::test]
async fn test_display_batch_id_archived_returns_none() {
    let store = setup_store_with_batches().await;
    let mut batch = Batch::new(1, 0);
    batch.status = BatchStatus::Archived;
    store.batches.lock().await.push(batch);
    add_task(&store, "1", 1, TaskStatus::Completed).await;

    assert_eq!(store.display_batch_id().await, None);
}

#[tokio::test]
async fn test_display_batch_id_active_returns_batch() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1, 0));
    add_task(&store, "1", 1, TaskStatus::Pending).await;

    assert_eq!(store.display_batch_id().await, Some(1));
}

// --- format_display_id ---

#[tokio::test]
async fn test_format_display_id_with_mapping() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1, 0));
    add_task(&store, "99", 1, TaskStatus::Pending).await;

    assert_eq!(store.format_display_id("99").await, "1");
}

#[tokio::test]
async fn test_format_display_id_fallback() {
    let store = setup_store_with_batches().await;
    // No task exists, fallback to raw id
    assert_eq!(store.format_display_id("42").await, "42");
}

// --- cross-batch consistency ---

#[tokio::test]
async fn test_cross_batch_display_numbers_independent() {
    let store = setup_store_with_batches().await;
    // Batch 0 (archived)
    let mut b0 = Batch::new(0, 0);
    b0.status = BatchStatus::Archived;
    store.batches.lock().await.push(b0);
    add_task(&store, "1", 0, TaskStatus::Completed).await;
    add_task(&store, "2", 0, TaskStatus::Completed).await;

    // Batch 1 (active)
    store.batches.lock().await.push(Batch::new(1, 0));
    add_task(&store, "8", 1, TaskStatus::Pending).await;
    add_task(&store, "9", 1, TaskStatus::Pending).await;

    // get_display_number works per-batch
    assert_eq!(store.get_display_number("8").await, Some(1));
    assert_eq!(store.get_display_number("9").await, Some(2));

    // resolve_display_id uses current batch (1)
    assert_eq!(store.resolve_display_id("1").await, Some("8".to_string()));
    assert_eq!(store.resolve_display_id("2").await, Some("9".to_string()));
}

// --- get_batch_display_map ---

#[tokio::test]
async fn test_get_batch_display_map_empty() {
    let store = setup_store_with_batches().await;
    let map = store.get_batch_display_map().await;
    assert!(map.is_empty());
}

#[tokio::test]
async fn test_get_batch_display_map_returns_sequential_numbers() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1, 0));
    add_task(&store, "8", 1, TaskStatus::Pending).await;
    add_task(&store, "9", 1, TaskStatus::InProgress).await;
    add_task(&store, "10", 1, TaskStatus::Completed).await;
    let map = store.get_batch_display_map().await;
    assert_eq!(map["8"], 1);
    assert_eq!(map["9"], 2);
    assert_eq!(map["10"], 3);
}

#[tokio::test]
async fn test_get_batch_display_map_excludes_deleted() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1, 0));
    add_task(&store, "1", 1, TaskStatus::Pending).await;
    add_task(&store, "2", 1, TaskStatus::Deleted).await;
    add_task(&store, "3", 1, TaskStatus::InProgress).await;
    let map = store.get_batch_display_map().await;
    assert_eq!(map.len(), 2);
    assert_eq!(map["1"], 1);
    assert_eq!(map["3"], 2);
}
