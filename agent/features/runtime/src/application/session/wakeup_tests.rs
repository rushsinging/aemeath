use super::*;

#[tokio::test]
async fn wakeup_signal_round_trips_and_coalesces() {
    let (notifier, mut waiter) = wakeup_channel();
    assert!(notifier.wakeup().is_ok(), "接收端存活时唤醒成功");
    assert!(notifier.wakeup().is_ok(), "多次唤醒可合流（信号语义）");

    // 信号语义：至少可取到一次（合流允许只取一次）。
    let mut received = 0;
    while waiter.try_wait().is_some() {
        received += 1;
    }
    assert!(received >= 1, "唤醒信号必须可被等待端取到");
}

#[tokio::test]
async fn waiter_wait_returns_none_when_notifier_dropped() {
    let (notifier, mut waiter) = wakeup_channel();
    drop(notifier);
    assert!(
        waiter.wait().await.is_none(),
        "发送端全部释放后等待端必须感知关闭（session 退出）"
    );
}

#[tokio::test]
async fn wait_blocks_until_wakeup_arrives() {
    let (notifier, waiter) = wakeup_channel();
    let consumer = {
        let waiter = std::sync::Arc::new(tokio::sync::Mutex::new(waiter));
        tokio::spawn(async move { waiter.lock().await.wait().await })
    };
    // 给 consumer 一点时间进入等待（无信号时不得提前返回）。
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(!consumer.is_finished(), "无唤醒信号时等待必须阻塞");
    notifier.wakeup().unwrap();
    assert_eq!(consumer.await.unwrap(), Some(()), "唤醒后等待必须返回信号");
}
