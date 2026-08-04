use std::time::Duration;

#[cfg(test)]
#[path = "terminal_tests.rs"]
mod tests;

/// Conversation 终态原因。实时事件与 Session Resume 必须投影为同一语义。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalCause {
    Completed,
    UserCancelled,
    RunTerminated,
}

const DONE_VERBS: [&str; 20] = [
    "Sautéed",
    "Baked",
    "Grilled",
    "Simmered",
    "Roasted",
    "Brewed",
    "Toasted",
    "Stewed",
    "Marinated",
    "Charred",
    "Poached",
    "Steamed",
    "Smoked",
    "Brûléed",
    "Flambéed",
    "Fermented",
    "Pickled",
    "Cured",
    "Seared",
    "Blanched",
];

fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

pub fn terminal_notice(cause: TerminalCause, duration: Option<Duration>) -> Option<String> {
    match cause {
        TerminalCause::Completed => {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let index = COUNTER.fetch_add(1, Ordering::Relaxed) % DONE_VERBS.len();
            let verb = DONE_VERBS.get(index).copied().unwrap_or(DONE_VERBS[0]);
            Some(duration.map_or_else(
                || format!("✻ {verb}"),
                |duration| format!("✻ {verb} for {}", format_duration(duration)),
            ))
        }
        TerminalCause::UserCancelled => Some(duration.map_or_else(
            || "✻ Cancelled".to_string(),
            |duration| format!("✻ Cancelled, ran {}", format_duration(duration)),
        )),
        TerminalCause::RunTerminated => Some("此 Run 已终止".to_string()),
    }
}
