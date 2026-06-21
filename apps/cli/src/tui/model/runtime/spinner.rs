//! Spinner 业务态（是否活跃 + 当前 phase）。动画细节 frame/verb 归 view_state（见 spec）。

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpinnerModel {
    pub active: bool,
    pub phase: Option<SpinnerPhase>,
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
        assert!(!model.active);
        assert_eq!(model.phase, None);
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
}
