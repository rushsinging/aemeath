use std::sync::Arc;

use context::application::main_session::SessionSwitchGate;
use tokio::sync::oneshot;

#[tokio::test]
async fn exclusive_waits_until_all_shared_holders_are_joined() {
    let gate = Arc::new(SessionSwitchGate::new());
    let first = gate.acquire_shared().await.expect("shared lease");
    let second = gate.acquire_shared().await.expect("shared lease");
    let (acquired_tx, mut acquired_rx) = oneshot::channel();

    let contender = {
        let gate = gate.clone();
        tokio::spawn(async move {
            let permit = gate
                .acquire_owned_exclusive()
                .await
                .expect("exclusive lease");
            let _ = acquired_tx.send(permit);
        })
    };

    assert!(
        acquired_rx.try_recv().is_err(),
        "shared holder 存活时不得取得 exclusive lease"
    );
    drop(first);
    assert!(
        acquired_rx.try_recv().is_err(),
        "必须等待全部 shared holder 释放"
    );
    drop(second);

    let exclusive = acquired_rx
        .await
        .expect("exclusive lease 应在 holders 清空后取得");
    assert!(
        gate.try_acquire_shared().is_err(),
        "exclusive lease 存活时不得 admission 新 Main Run"
    );
    drop(exclusive);
    contender.await.expect("exclusive contender task");
    assert!(
        gate.try_acquire_shared().is_ok(),
        "exclusive lease 释放后应恢复 admission"
    );
}

#[tokio::test]
async fn owned_shared_permit_can_cross_spawned_holder_lifetime() {
    let gate = Arc::new(SessionSwitchGate::new());
    let shared = gate.acquire_shared().await.expect("shared lease");
    let (release_tx, release_rx) = oneshot::channel::<()>();

    let holder = tokio::spawn(async move {
        let _lease = shared;
        let _ = release_rx.await;
    });

    let exclusive = {
        let gate = gate.clone();
        tokio::spawn(async move {
            gate.acquire_owned_exclusive()
                .await
                .expect("exclusive lease")
        })
    };
    assert!(
        !exclusive.is_finished(),
        "派生 holder 持 lease 时 exclusive 不得完成"
    );

    release_tx.send(()).expect("release holder");
    holder.await.expect("holder join");
    drop(exclusive.await.expect("exclusive join"));
}
