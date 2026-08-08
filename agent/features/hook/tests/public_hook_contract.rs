#![cfg(unix)]

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use hook::{
    HookDirective, HookDispatchContext, HookExecutionStatus, HookInvocation, HookPort, HookReason,
    PreToolUseInput,
};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
use share::config::Config;
use tokio_util::sync::CancellationToken;

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn dispatcher_for(command: String, timeout: u64) -> hook::Dispatcher {
    let config = Config {
        hooks: HooksConfig {
            events: HashMap::from([(
                HookEvent::PreToolUse,
                vec![HookEntry {
                    matcher: "Bash".to_string(),
                    command,
                    timeout,
                }],
            )]),
            ..HooksConfig::default()
        },
        ..Config::default()
    };

    hook::build_dispatcher(&ConfigSnapshot::new(config)).expect("公开配置入口应构造 Dispatcher")
}

fn invocation() -> HookInvocation {
    HookInvocation::PreToolUse(PreToolUseInput {
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({"command": "printf contract"}),
    })
}

async fn dispatch(dispatcher: &hook::Dispatcher, cwd: &Path) -> hook::HookOutcome {
    dispatcher
        .dispatch_at(
            invocation(),
            HookDispatchContext::new(cwd),
            &CancellationToken::new(),
        )
        .await
}

#[tokio::test]
async fn public_port_treats_nonzero_exit_as_single_business_block() {
    let temp = tempfile::tempdir().expect("创建临时目录");
    let attempts = temp.path().join("block-attempts");
    let command = format!(
        "printf 'attempt\\n' >> {}; exit 127",
        shell_quote(&attempts)
    );
    let outcome = dispatch(&dispatcher_for(command, 2), temp.path()).await;

    assert!(matches!(
        outcome.directive,
        HookDirective::Block {
            reason: HookReason::ExitCode { code: 127, .. }
        }
    ));
    assert_eq!(outcome.executions.len(), 1);
    assert_eq!(
        std::fs::read_to_string(attempts).expect("读取执行次数"),
        "attempt\n"
    );
}

#[tokio::test]
async fn public_port_retries_protocol_failure_three_times_then_continues() {
    let temp = tempfile::tempdir().expect("创建临时目录");
    let attempts = temp.path().join("failure-attempts");
    let command = format!(
        "printf 'attempt\\n' >> {}; printf '{{'",
        shell_quote(&attempts)
    );
    let outcome = dispatch(&dispatcher_for(command, 2), temp.path()).await;

    assert_eq!(outcome.directive, HookDirective::Continue);
    assert_eq!(outcome.executions.len(), 3);
    assert!(outcome.executions.iter().all(|execution| matches!(
        execution.status,
        HookExecutionStatus::ExecutionFailed { .. }
    )));
    assert_eq!(
        std::fs::read_to_string(attempts).expect("读取执行次数"),
        "attempt\nattempt\nattempt\n"
    );
}

#[tokio::test]
async fn public_port_injects_approved_invocation_environment() {
    let temp = tempfile::tempdir().expect("创建临时目录");
    let command = "printf '%s' \"${AEMEATH_PROJECT_DIR-missing}\"".to_string();

    let outcome = dispatch(&dispatcher_for(command, 2), temp.path()).await;

    assert_eq!(outcome.executions.len(), 1);
    assert_eq!(
        outcome.executions[0].stdout,
        temp.path().display().to_string()
    );
}

#[tokio::test]
async fn public_port_reaps_background_process_group_after_shell_exit() {
    let temp = tempfile::tempdir().expect("创建临时目录");
    let marker = temp.path().join("background-pid");
    let command = format!(
        "sleep 30 & child=$!; printf '%s' $child > {}; exit 0",
        shell_quote(&marker)
    );
    let started = Instant::now();
    let outcome = dispatch(&dispatcher_for(command, 10), temp.path()).await;
    let elapsed = started.elapsed();
    let child_pid = std::fs::read_to_string(marker)
        .expect("读取后台进程 PID")
        .parse::<libc::pid_t>()
        .expect("解析后台进程 PID");

    assert_eq!(outcome.directive, HookDirective::Continue);
    assert!(
        elapsed < Duration::from_secs(3),
        "后台进程不得让公开 dispatch 等待到 timeout，实际耗时 {elapsed:?}"
    );

    let reap_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let probe_result = unsafe { libc::kill(child_pid, 0) };
        if probe_result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            break;
        }
        assert!(
            Instant::now() < reap_deadline,
            "后台进程必须在有界时间内回收"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
