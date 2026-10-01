use super::*;
use memory::api::search::{MemoryRetrievalMode, MemorySearchQuery};
use std::sync::atomic::{AtomicUsize, Ordering};

fn service() -> Arc<dyn MemoryPort> {
    Arc::new(NoOpMemory)
}

#[test]
fn main_views_share_the_active_arc() {
    let active = service();
    let wiring = ActiveMemoryWiring::new(PreparedMemory::new("project-a", active.clone()));

    let views = wiring.main_views();

    assert!(Arc::ptr_eq(&active, &views.context));
    assert!(Arc::ptr_eq(&views.context, &views.tools));
    assert!(Arc::ptr_eq(&views.context, &views.runtime));
    assert!(Arc::ptr_eq(&views.context, &views.reflection));
}

#[tokio::test]
async fn prepare_does_not_change_active_until_install() {
    let first = service();
    let second = service();
    let mut wiring = ActiveMemoryWiring::new(PreparedMemory::new("project-a", first.clone()));

    let prepared = wiring
        .prepare("project-b", || async { Ok::<_, ()>(second.clone()) })
        .await
        .unwrap();

    assert!(Arc::ptr_eq(&wiring.main_views().context, &first));
    assert!(!Arc::ptr_eq(
        &wiring.main_views().context,
        prepared.memory()
    ));

    wiring.install(prepared);
    assert_eq!(wiring.identity(), &"project-b");
    assert!(Arc::ptr_eq(&wiring.main_views().context, &second));
}

#[tokio::test]
async fn preparing_the_active_identity_reuses_arc_without_opening() {
    let opens = AtomicUsize::new(0);
    let active = service();
    let wiring = ActiveMemoryWiring::new(PreparedMemory::new("project-a", active.clone()));

    let prepared = wiring
        .prepare("project-a", || async {
            opens.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>(service())
        })
        .await
        .unwrap();

    assert_eq!(opens.load(Ordering::SeqCst), 0);
    assert!(Arc::ptr_eq(prepared.memory(), &active));
}

#[test]
fn sub_disabled_is_noop_and_shared_reuses_active_without_opening() {
    let active = service();
    let wiring = ActiveMemoryWiring::new(PreparedMemory::new("project-a", active.clone()));

    let disabled = wiring.derive_sub(MemoryMode::Disabled);
    let shared = wiring.derive_sub(MemoryMode::Shared);

    assert!(!Arc::ptr_eq(&disabled, &active));
    assert!(Arc::ptr_eq(&shared, &active));
    let query = MemorySearchQuery {
        text: String::new(),
        limit: 1,
        layer: None,
        category: None,
        include_archive: false,
        now: 0,
    };
    assert_eq!(disabled.search(&query).mode, MemoryRetrievalMode::Disabled);
}
