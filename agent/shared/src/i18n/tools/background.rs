//! 后台进程工具族文案（#252）。

/// BackgroundProcessList description。
pub fn background_process_list(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            "列出活动与近期的后台进程（超阈值自动转后台的 tool call）：进程 id、工具、状态、时长。占位 tool_result 标注\"已转后台运行\"的进程用本工具跟进。"
        }
        _ => {
            "List active and recent background processes (tool calls moved to background after exceeding the foreground threshold): process id, tool, state, and duration. Follow up on tool results marked as running in the background with this tool."
        }
    }
}

/// BackgroundProcessStatus description。
pub fn background_process_status(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            "按进程 id 查询单个后台进程详情：状态、终态、deadline 剩余与日志字节数。"
        }
        _ => {
            "Retrieve one background process's detail by process id: state, terminal outcome, remaining deadline, and log byte count."
        }
    }
}

/// BackgroundProcessLogs description。
pub fn background_process_logs(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"读取后台进程日志：运行中与完成后皆可查。缺省读尾部；携带上次返回的 cursor 只读增量（增量游标，多次读取幂等、不破坏后续通知）。"#
        }
        _ => {
            r#"Read a background process's log: works while running and after completion. Omit cursor for the tail; pass the previously returned cursor to read only new output (incremental cursor; reads are idempotent and never interfere with completion notifications)."#
        }
    }
}

/// BackgroundProcessStop description。
pub fn background_process_stop(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            "请求停止一个后台进程：发出取消信号；真实终态（成功/失败/取消不确定）由执行体收口后经完成通知或后续查询可见。"
        }
        _ => {
            "Request to stop a background process: sends the cancel signal; the real terminal outcome (succeeded / failed / cancellation unconfirmed) arrives via the completion notification or a later query after the process winds down."
        }
    }
}
