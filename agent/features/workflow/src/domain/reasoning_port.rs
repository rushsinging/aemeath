use super::reasoning_graph::{ReasoningGraph, ReasoningNode, ReasoningSignal};
use share::reasoning::ReasoningLevel;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReasoningObservation {
    pub previous: ReasoningNode,
    pub current: ReasoningNode,
    pub requested: ReasoningLevel,
}

impl ReasoningObservation {
    pub fn changed(self) -> bool {
        self.previous != self.current
    }
}

pub trait ReasoningPort: Send + Sync {
    fn observe(&self, signal: ReasoningSignal) -> ReasoningObservation;
    fn current_requested_level(&self) -> ReasoningLevel;
    fn set_level(&self, level: ReasoningLevel) -> ReasoningLevel;
    fn reset_default_level(&self, level: ReasoningLevel) -> ReasoningLevel;
}

/// #920 Off 硬门：当 level 为 Off 时记为「粘性覆盖」，requested 冻结在 Off；
/// 任何非 Off 的值都会清除覆盖，恢复自适应（requested 跟随 graph effort）。
struct AdaptiveState {
    graph: ReasoningGraph,
    requested: ReasoningLevel,
    /// 仅在 level == Off 时为 `Some(Off)`，其余情况为 `None`。
    off_override: Option<ReasoningLevel>,
}

pub struct AdaptiveReasoningPort {
    state: Mutex<AdaptiveState>,
}

impl AdaptiveReasoningPort {
    /// 创建自适应 reasoning port。
    ///
    /// 不再 clamp `initial`——调用方传入的值即初始 requested。
    /// 若 `initial == Off` 则建立 Off 硬门（requested 冻结，直到非 Off 值解锁）。
    pub fn new(initial: ReasoningLevel) -> Self {
        Self {
            state: Mutex::new(AdaptiveState {
                graph: ReasoningGraph::new(),
                requested: initial,
                off_override: matches!(initial, ReasoningLevel::Off).then_some(initial),
            }),
        }
    }
}

impl ReasoningPort for AdaptiveReasoningPort {
    fn observe(&self, signal: ReasoningSignal) -> ReasoningObservation {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let previous = state.graph.current_node();
        // Graph 始终运行，无条件消费信号推进节点。
        state.graph.transition(signal);
        // 仅当未处于 Off 硬门时，requested 跟随当前节点 effort（自适应）。
        if state.off_override.is_none() {
            state.requested = state.graph.current_effort();
        }
        ReasoningObservation {
            previous,
            current: state.graph.current_node(),
            requested: state.requested,
        }
    }

    fn current_requested_level(&self) -> ReasoningLevel {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .requested
    }

    fn set_level(&self, level: ReasoningLevel) -> ReasoningLevel {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        // Off 建立硬门；非 Off 清除覆盖、恢复自适应。
        state.off_override = matches!(level, ReasoningLevel::Off).then_some(level);
        state.requested = level;
        level
    }

    fn reset_default_level(&self, level: ReasoningLevel) -> ReasoningLevel {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        // reset default 同样以 Off 为硬门分界：非 Off 不留粘性覆盖，
        // 下一次 observe 即恢复跟随 graph effort。
        state.off_override = matches!(level, ReasoningLevel::Off).then_some(level);
        state.requested = level;
        level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_observation_reports_transition_and_requested_level() {
        let port = AdaptiveReasoningPort::new(ReasoningLevel::Medium);

        let observation = port.observe(ReasoningSignal::UserMessage {
            text: "fix typo".to_string(),
            turn_count: 1,
        });

        assert_eq!(observation.previous, ReasoningNode::Idle);
        assert_eq!(observation.current, ReasoningNode::Explore);
        assert_eq!(observation.requested, ReasoningLevel::Medium);
        assert!(observation.changed());
    }

    #[test]
    fn off_hard_door_freezes_requested_until_non_off_unlocks_adaptive() {
        // 非 Off 初始：无覆盖，requested 跟随 graph。
        let port = AdaptiveReasoningPort::new(ReasoningLevel::Medium);

        // 建立 Off 硬门。
        assert_eq!(port.set_level(ReasoningLevel::Off), ReasoningLevel::Off);
        let observation = port.observe(ReasoningSignal::ToolCompleted {
            tool_name: "Edit".to_string(),
            bash_command: None,
            is_error: false,
            declared_phase: Some("execute".to_string()),
        });
        // graph 仍在推进节点，但 requested 冻结在 Off。
        assert_eq!(observation.current, ReasoningNode::Execute);
        assert_eq!(observation.requested, ReasoningLevel::Off);

        // 非 Off 解锁：清除覆盖，恢复自适应。
        assert_eq!(
            port.set_level(ReasoningLevel::Medium),
            ReasoningLevel::Medium
        );
        let observation = port.observe(ReasoningSignal::ToolCompleted {
            tool_name: "Read".to_string(),
            bash_command: None,
            is_error: false,
            declared_phase: Some("plan".to_string()),
        });
        // 跟随 Plan 默认 effort = Max（无 user_max clamp）。
        assert_eq!(observation.requested, ReasoningLevel::Max);
    }

    #[test]
    fn model_default_reset_replaces_previous_requested_without_sticky_override() {
        let port = AdaptiveReasoningPort::new(ReasoningLevel::Low);
        assert_eq!(port.set_level(ReasoningLevel::Off), ReasoningLevel::Off);
        // reset 到非 Off 默认值：不留粘性覆盖。
        assert_eq!(
            port.reset_default_level(ReasoningLevel::Medium),
            ReasoningLevel::Medium
        );

        let observation = port.observe(ReasoningSignal::ToolCompleted {
            tool_name: "Read".to_string(),
            bash_command: None,
            is_error: false,
            declared_phase: Some("plan".to_string()),
        });
        // 无覆盖 → 跟随 Plan 默认 effort = Max。
        assert_eq!(observation.requested, ReasoningLevel::Max);
    }
}
