//! 后台任务工具族文案（#252）。

/// BackgroundTaskList description。
pub fn background_task_list(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            "列出活动与近期的后台任务（超阈值自动转后台的 tool call）：任务 id、工具、状态、时长。占位 tool_result 标注\"已转后台运行\"的任务用本工具跟进。"
        }
        _ => {
            "List active and recent background tasks (tool calls moved to background after exceeding the foreground threshold): task id, tool, state, and duration. Follow up on tool results marked as running in the background with this tool."
        }
    }
}

/// BackgroundTaskStatus description。
pub fn background_task_status(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            "按任务 id 查询单个后台任务详情：状态、终态、deadline 剩余与日志字节数。"
        }
        _ => {
            "Retrieve one background task's detail by task id: state, terminal outcome, remaining deadline, and log byte count."
        }
    }
}

/// BackgroundTaskLogs description。
pub fn background_task_logs(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"读取后台任务日志：运行中与完成后皆可查。缺省读尾部；携带上次返回的 cursor 只读增量（增量游标，多次读取幂等、不破坏后续通知）。"#
        }
        _ => {
            r#"Read a background task's log: works while running and after completion. Omit cursor for the tail; pass the previously returned cursor to read only new output (incremental cursor; reads are idempotent and never interfere with completion notifications)."#
        }
    }
}

/// BackgroundTaskStop description。
pub fn background_task_stop(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            "请求停止一个后台任务：发出取消信号；真实终态（成功/失败/取消不确定）由执行体收口后经完成通知或后续查询可见。"
        }
        _ => {
            "Request to stop a background task: sends the cancel signal; the real terminal outcome (succeeded / failed / cancellation unconfirmed) arrives via the completion notification or a later query after the task winds down."
        }
    }
}
