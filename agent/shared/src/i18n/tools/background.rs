//! 后台任务工具文案（background_tasks 的 description，#252）。

/// BackgroundTasks description。
pub fn background_tasks(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"查询或停止后台任务（超阈值自动转后台的 tool call）。action=list 列出任务（id/工具/状态/时长）；status 查单任务详情；logs 查任务日志（携带上次返回的 cursor 只读增量，缺省读尾部，运行中与完成后皆可）；stop 请求停止（发取消信号，真实终态稍后经通知或查询可见）。占位 tool_result 标注"已转后台运行"的任务用本工具跟进。"#
        }
        _ => {
            r#"Inspect or stop background tasks (tool calls moved to background after exceeding the foreground threshold). action=list shows tasks (id/tool/state/duration); status shows one task's detail; logs reads task output (pass the previously returned cursor for incremental reads, omit for the tail; works while running and after completion); stop requests cancellation (sends the cancel signal; the real terminal state arrives via notification or a later query). Follow up on tool results marked as running in the background with this tool."#
        }
    }
}
