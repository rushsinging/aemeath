//! Spinner 业务态（phase + running_tool_count）。动画细节 frame/verb 归 view_state。

/// Spinner 业务真相。`phase = None` 表示不活跃（停止）。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpinnerModel {
    pub phase: Option<SpinnerPhase>,
    /// 运行中 tool call 计数器，由 intent update 增减（tool start +1 / tool result -1）。
    pub running_tool_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpinnerPhase {
    Thinking,
    Generating,
    AgentWorking,
    Reflecting,
    Compacting,
    CallingTool(String),
    CallingTools {
        remaining: usize,
    },
    Hook {
        event: String,
        detail: String,
        outcome: HookOutcome,
    },
}

impl SpinnerPhase {
    /// 将 phase 语义格式化为显示文案。
    pub fn text(&self) -> String {
        match self {
            Self::Thinking => "Thinking...".to_string(),
            Self::Generating => "Generating...".to_string(),
            Self::AgentWorking => "Agent working...".to_string(),
            Self::Reflecting => "Reflecting...".to_string(),
            Self::Compacting => "Compacting...".to_string(),
            Self::CallingTool(name) => format!("Calling {name}..."),
            Self::CallingTools { remaining } => {
                format!("Calling tools... ({remaining} running)")
            }
            Self::Hook {
                event,
                detail,
                outcome,
            } => match outcome {
                HookOutcome::Running => format!("Hook {event}: {detail}"),
                HookOutcome::Blocked => format!("Hook {event} blocked"),
                HookOutcome::Done => format!("Hook {event} done"),
                HookOutcome::Failed => format!("Hook {event} failed: {detail}"),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookOutcome {
    Running,
    Blocked,
    Done,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_model_default_is_inactive() {
        let model = SpinnerModel::default();
        assert_eq!(model.phase, None);
        assert_eq!(model.running_tool_count, 0);
    }

    #[test]
    fn test_spinner_phase_simple_variants() {
        assert_eq!(SpinnerPhase::Thinking, SpinnerPhase::Thinking);
        assert_ne!(SpinnerPhase::Thinking, SpinnerPhase::Generating);
        assert_eq!(
            SpinnerPhase::CallingTool("read".to_string()),
            SpinnerPhase::CallingTool("read".to_string())
        );
        assert_eq!(
            SpinnerPhase::CallingTools { remaining: 2 },
            SpinnerPhase::CallingTools { remaining: 2 }
        );
    }

    #[test]
    fn test_spinner_phase_hook_variant() {
        let phase = SpinnerPhase::Hook {
            event: "PreToolUse".to_string(),
            detail: "lint".to_string(),
            outcome: HookOutcome::Running,
        };
        assert_eq!(
            phase,
            SpinnerPhase::Hook {
                event: "PreToolUse".to_string(),
                detail: "lint".to_string(),
                outcome: HookOutcome::Running,
            }
        );
        assert_ne!(HookOutcome::Running, HookOutcome::Blocked);
    }

    #[test]
    fn test_spinner_phase_text() {
        assert_eq!(SpinnerPhase::Thinking.text(), "Thinking...");
        assert_eq!(SpinnerPhase::Generating.text(), "Generating...");
        assert_eq!(
            SpinnerPhase::CallingTool("Read".to_string()).text(),
            "Calling Read..."
        );
        assert_eq!(
            SpinnerPhase::CallingTools { remaining: 3 }.text(),
            "Calling tools... (3 running)"
        );
        assert_eq!(
            SpinnerPhase::Hook {
                event: "PreToolUse".to_string(),
                detail: "lint".to_string(),
                outcome: HookOutcome::Running
            }
            .text(),
            "Hook PreToolUse: lint"
        );
        assert_eq!(
            SpinnerPhase::Hook {
                event: "PreToolUse".to_string(),
                detail: "lint".to_string(),
                outcome: HookOutcome::Blocked
            }
            .text(),
            "Hook PreToolUse blocked"
        );
    }
}
