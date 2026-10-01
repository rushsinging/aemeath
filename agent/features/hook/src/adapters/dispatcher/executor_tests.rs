use super::*;

#[test]
fn spawn_failure_preserves_os_detail_in_execution_message() {
    let fault = map_process_failure(ProcessFailure {
        kind: ProcessFailureKind::Spawn,
        message: "启动 hook 命令失败: Too many open files (os error 24)".to_string(),
    });

    assert_eq!(
        fault.message(),
        "hook 子进程启动失败: 启动 hook 命令失败: Too many open files (os error 24)"
    );
}
