use super::*;

#[cfg(unix)]
#[tokio::test]
async fn execute_hook_with_cancel_interrupts_running_process() {
    let runner = HookRunner::empty();
    let hook = HookEntry {
        matcher: String::new(),
        command: "sleep 30".to_string(),
        timeout: 60,
    };
    let input = HookInput {
        event: HookEvent::Stop,
        data: HookData::Stop(crate::adapters::legacy::data::StopHookData { turns: 1 }),
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    let cancel_task = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::task::yield_now().await;
        cancel_task.cancel();
    });

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        runner.execute_hook_with_cancel(&hook, &input, Path::new("."), &cancel),
    )
    .await
    .expect("取消必须中断并回收在途 Hook 进程");

    canceller.await.unwrap();
    assert!(result.error.is_some_and(|error| error.contains("已取消")));
    assert_eq!(result.exit_code, None);
}

#[cfg(unix)]
#[tokio::test]
async fn truncated_stdout_is_reported_and_cannot_be_parsed_as_json() {
    let runner = HookRunner::empty();
    let hook = HookEntry {
        matcher: String::new(),
        command: "printf '{\"continue\":true,\"padding\":\"'; yes x | head -c 20000; printf '\"}'"
            .to_string(),
        timeout: 5,
    };
    let input = HookInput {
        event: HookEvent::Stop,
        data: HookData::Stop(crate::adapters::legacy::data::StopHookData { turns: 1 }),
    };

    let result = runner.execute_hook(&hook, &input, Path::new(".")).await;

    assert!(result
        .error
        .as_ref()
        .is_some_and(|error| error.contains("已截断")));
    assert!(result.output.is_empty());
    assert!(result.parse_json_output().is_none());
}
