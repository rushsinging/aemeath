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
async fn truncated_stdout_on_success_keeps_prefix_and_neither_blocks_nor_errors() {
    // #1220：hook exit 0 + stdout 超 DEFAULT_OUTPUT_LIMIT(8192) 时，截断是信息性事件。
    // MUST NOT 判 block、MUST NOT 设 error、MUST 保留截断后的 stdout 前缀（足以解析 JSON）。
    let runner = HookRunner::empty();
    let hook = HookEntry {
        matcher: String::new(),
        // 完整 JSON 放在最前面（落在 8192 截断窗口内），后面跟超长 padding 触发截断。
        command: "printf '{\"continue\":true}'; yes x | head -c 20000".to_string(),
        timeout: 5,
    };
    let input = HookInput {
        event: HookEvent::Stop,
        data: HookData::Stop(crate::adapters::legacy::data::StopHookData { turns: 1 }),
    };

    let result = runner.execute_hook(&hook, &input, Path::new(".")).await;

    assert!(!result.blocked, "exit 0 时截断不应判 block");
    assert!(
        result.error.is_none(),
        "截断不应设 error，实际: {:?}",
        result.error
    );
    assert!(
        result.output_truncated,
        "stdout 超 limit 应标记 output_truncated=true"
    );
    assert!(
        !result.output.is_empty(),
        "截断后 MUST 保留 stdout 前缀，NEVER 清空"
    );
    assert!(
        result.output.starts_with("{\"continue\":true}"),
        "保留的前缀 MUST 包含 hook 的 JSON directive 开头，实际开头: {:?}",
        &result.output[..result.output.len().min(40)]
    );
    assert_eq!(result.exit_code, Some(0));
}

#[cfg(unix)]
#[tokio::test]
async fn truncated_stdout_on_nonzero_exit_reports_real_exit_code_not_truncation() {
    // #1220：hook exit 非零 + stdout 超限时，error MUST 反映真实 exit code，
    // 而非被"输出截断"覆盖。output_truncated 仍标记 true。
    let runner = HookRunner::empty();
    let hook = HookEntry {
        matcher: String::new(),
        command: "yes x | head -c 20000; exit 3".to_string(),
        timeout: 5,
    };
    let input = HookInput {
        event: HookEvent::Stop,
        data: HookData::Stop(crate::adapters::legacy::data::StopHookData { turns: 1 }),
    };

    let result = runner.execute_hook(&hook, &input, Path::new(".")).await;

    assert!(result.blocked, "exit 非零应判 block");
    assert!(
        result.output_truncated,
        "stdout 超 limit 应标记 output_truncated=true"
    );
    assert!(
        result
            .error
            .as_ref()
            .is_some_and(|error| error.contains("exit code 3")),
        "error MUST 含真实 exit code，实际: {:?}",
        result.error
    );
    assert!(
        !result
            .error
            .as_ref()
            .is_some_and(|error| error.contains("截断")),
        "error MUST NOT 被'截断'覆盖"
    );
}
