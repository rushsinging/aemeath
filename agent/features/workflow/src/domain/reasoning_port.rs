use super::reasoning_graph::{GraphRuntimeConfig, ReasoningGraph, ReasoningNode, ReasoningSignal};
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

struct AdaptiveState {
    graph: ReasoningGraph,
    requested: ReasoningLevel,
    manual_override: Option<ReasoningLevel>,
}

pub struct AdaptiveReasoningPort {
    state: Mutex<AdaptiveState>,
    user_max: ReasoningLevel,
}

impl AdaptiveReasoningPort {
    pub fn new(config: GraphRuntimeConfig, initial: ReasoningLevel) -> Self {
        let user_max = config.max_reasoning;
        let initial = initial.clamped_to(user_max);
        Self {
            state: Mutex::new(AdaptiveState {
                graph: ReasoningGraph::new(config),
                requested: initial,
                manual_override: matches!(initial, ReasoningLevel::Off).then_some(initial),
            }),
            user_max,
        }
    }

    fn clamp(&self, desired: ReasoningLevel) -> ReasoningLevel {
        desired.clamped_to(self.user_max)
    }
}

impl ReasoningPort for AdaptiveReasoningPort {
    fn observe(&self, signal: ReasoningSignal) -> ReasoningObservation {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let previous = state.graph.current_node();
        if state.graph.enabled() {
            state.graph.transition(signal);
            if state.manual_override.is_none() {
                state.requested = self.clamp(state.graph.current_effort());
            }
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
        let requested = self.clamp(level);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.manual_override = matches!(requested, ReasoningLevel::Off).then_some(requested);
        state.requested = requested;
        requested
    }

    fn reset_default_level(&self, level: ReasoningLevel) -> ReasoningLevel {
        let requested = self.clamp(level);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.manual_override = matches!(requested, ReasoningLevel::Off).then_some(requested);
        state.requested = requested;
        requested
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(max_reasoning: ReasoningLevel) -> GraphRuntimeConfig {
        GraphRuntimeConfig {
            enabled: true,
            max_reasoning,
            explore_effort: None,
            plan_effort: None,
            execute_effort: None,
            verify_effort: None,
        }
    }

    #[test]
    fn adaptive_clamps_initial_and_observed_desired_to_user_max() {
        for (maximum, expected) in [
            (ReasoningLevel::Off, ReasoningLevel::Off),
            (ReasoningLevel::Low, ReasoningLevel::Low),
            (ReasoningLevel::Medium, ReasoningLevel::Medium),
            (ReasoningLevel::High, ReasoningLevel::High),
            (ReasoningLevel::Xhigh, ReasoningLevel::Xhigh),
            (ReasoningLevel::Max, ReasoningLevel::Max),
        ] {
            let port = AdaptiveReasoningPort::new(config(maximum), ReasoningLevel::Max);
            assert_eq!(port.current_requested_level(), expected);

            let observation = port.observe(ReasoningSignal::UserMessage {
                text: "请设计新的架构".to_string(),
                turn_count: 1,
            });
            assert_eq!(observation.requested, expected);
            assert_eq!(port.current_requested_level(), expected);
        }
    }

    #[test]
    fn adaptive_observation_reports_transition_and_requested_level() {
        let port = AdaptiveReasoningPort::new(config(ReasoningLevel::Max), ReasoningLevel::Medium);

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
    fn off_override_blocks_graph_until_thinking_is_enabled_again() {
        let port = AdaptiveReasoningPort::new(config(ReasoningLevel::High), ReasoningLevel::Medium);

        assert_eq!(port.set_level(ReasoningLevel::Off), ReasoningLevel::Off);
        let observation = port.observe(ReasoningSignal::ToolCompleted {
            tool_name: "Edit".to_string(),
            bash_command: None,
            is_error: false,
            declared_phase: Some("execute".to_string()),
        });
        assert_eq!(observation.current, ReasoningNode::Execute);
        assert_eq!(observation.requested, ReasoningLevel::Off);

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
        assert_eq!(observation.requested, ReasoningLevel::High);
    }

    #[test]
    fn model_default_reset_replaces_previous_requested_without_sticky_override() {
        let port = AdaptiveReasoningPort::new(config(ReasoningLevel::High), ReasoningLevel::Low);
        assert_eq!(port.set_level(ReasoningLevel::Off), ReasoningLevel::Off);
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
        assert_eq!(observation.requested, ReasoningLevel::High);
    }

    #[test]
    fn disabled_graph_keeps_initial_requested_until_manual_override() {
        let mut disabled = config(ReasoningLevel::High);
        disabled.enabled = false;
        let port = AdaptiveReasoningPort::new(disabled, ReasoningLevel::Medium);

        let observation = port.observe(ReasoningSignal::UserMessage {
            text: "请设计新的架构".to_string(),
            turn_count: 1,
        });
        assert_eq!(observation.previous, ReasoningNode::Idle);
        assert_eq!(observation.current, ReasoningNode::Idle);
        assert_eq!(observation.requested, ReasoningLevel::Medium);
        assert!(!observation.changed());

        assert_eq!(port.set_level(ReasoningLevel::Max), ReasoningLevel::High);
    }
}
