use crate::application::subagent::ToolCall;
use serde_json::Value;
use std::collections::VecDeque;

const RECENT_TOOL_CALL_LIMIT: usize = 64;
const CONSECUTIVE_TOOL_CALL_SOFT_LIMIT: usize = 3;
const CONSECUTIVE_TOOL_CALL_HARD_LIMIT: usize = 5;
const PERIOD_MIN_LEN: usize = 2;
const PERIOD_MAX_LEN: usize = 5;
const PERIOD_REPEAT_LIMIT: usize = 3;
const TOOL_FUSE_HARD_PAUSE_LIMIT: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolFuseDecision {
    Allow,
    SoftBlock { reason: String },
    HardPause { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolCallFingerprint {
    tool_name: String,
    normalized_input: String,
}

impl ToolCallFingerprint {
    fn from_call(call: &ToolCall) -> Self {
        Self {
            tool_name: call.name.clone(),
            normalized_input: normalize_json(&call.input),
        }
    }

    fn summary(&self) -> String {
        const MAX_INPUT_SUMMARY_CHARS: usize = 160;
        let mut input = self.normalized_input.clone();
        if input.chars().count() > MAX_INPUT_SUMMARY_CHARS {
            input = input
                .chars()
                .take(MAX_INPUT_SUMMARY_CHARS)
                .collect::<String>();
            input.push_str("...");
        }
        format!("{}({})", self.tool_name, input)
    }
}

#[derive(Debug, Default)]
pub(crate) struct ToolCallFuse {
    recent: VecDeque<ToolCallFingerprint>,
    blocked_count: usize,
}

impl ToolCallFuse {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn inspect(&mut self, call: &ToolCall) -> ToolFuseDecision {
        let fingerprint = ToolCallFingerprint::from_call(call);
        self.recent.push_back(fingerprint.clone());
        while self.recent.len() > RECENT_TOOL_CALL_LIMIT {
            self.recent.pop_front();
        }

        let consecutive = self.consecutive_count(&fingerprint);
        let periodic = self.periodic_repeat();
        let soft_reason = if consecutive >= CONSECUTIVE_TOOL_CALL_SOFT_LIMIT {
            Some(format!(
                "repeated tool call detected: {} appeared {consecutive} consecutive times",
                fingerprint.summary()
            ))
        } else if let Some((period_len, repeats, sequence)) = periodic {
            Some(format!(
                "periodic tool call loop detected: period_len={period_len}, repeats={repeats}, sequence={}",
                sequence
                    .iter()
                    .map(ToolCallFingerprint::summary)
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ))
        } else {
            None
        };

        let Some(reason) = soft_reason else {
            return ToolFuseDecision::Allow;
        };

        self.blocked_count += 1;
        log::warn!(
            target: crate::LOG_TARGET,
            "tool call fuse triggered: tool={}, reason={}, blocked_count={}",
            fingerprint.tool_name,
            reason,
            self.blocked_count,
        );

        if consecutive >= CONSECUTIVE_TOOL_CALL_HARD_LIMIT
            || self.blocked_count >= TOOL_FUSE_HARD_PAUSE_LIMIT
        {
            ToolFuseDecision::HardPause { reason }
        } else {
            ToolFuseDecision::SoftBlock { reason }
        }
    }

    fn consecutive_count(&self, fingerprint: &ToolCallFingerprint) -> usize {
        self.recent
            .iter()
            .rev()
            .take_while(|recent| *recent == fingerprint)
            .count()
    }

    fn periodic_repeat(&self) -> Option<(usize, usize, Vec<ToolCallFingerprint>)> {
        for period_len in PERIOD_MIN_LEN..=PERIOD_MAX_LEN {
            let required = period_len * PERIOD_REPEAT_LIMIT;
            if self.recent.len() < required {
                continue;
            }
            let forward = self.recent.iter().cloned().collect::<Vec<_>>();
            let pattern = &forward[forward.len() - period_len..];
            let start = forward.len() - required;
            if forward[start..]
                .chunks(period_len)
                .all(|chunk| chunk == pattern)
            {
                return Some((period_len, PERIOD_REPEAT_LIMIT, pattern.to_vec()));
            }
        }
        None
    }
}

fn normalize_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => serde_json::to_string(v).unwrap_or_else(|_| "\"\"".to_string()),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(normalize_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(left, _)| *left);
            format!(
                "{{{}}}",
                entries
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string()),
                        normalize_json(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdk::ids::ToolCallId;

    fn call(name: &str, input: Value) -> ToolCall {
        ToolCall {
            id: ToolCallId::new_v7(),
            provider_id: format!("provider-{name}"),
            name: name.to_string(),
            index: 0,
            input,
        }
    }

    #[test]
    fn blocks_consecutive_identical_tool_calls() {
        let mut fuse = ToolCallFuse::new();
        let tool_call = call("Read", serde_json::json!({"file_path":"a.rs","limit":100}));

        assert_eq!(fuse.inspect(&tool_call), ToolFuseDecision::Allow);
        assert_eq!(fuse.inspect(&tool_call), ToolFuseDecision::Allow);
        assert!(matches!(
            fuse.inspect(&tool_call),
            ToolFuseDecision::SoftBlock { .. }
        ));
    }

    #[test]
    fn escalates_consecutive_identical_tool_calls_to_hard_pause() {
        let mut fuse = ToolCallFuse::new();
        let tool_call = call("Read", serde_json::json!({"file_path":"a.rs"}));

        for _ in 0..4 {
            let _ = fuse.inspect(&tool_call);
        }
        assert!(matches!(
            fuse.inspect(&tool_call),
            ToolFuseDecision::HardPause { .. }
        ));
    }

    #[test]
    fn blocks_short_periodic_tool_call_loop() {
        let mut fuse = ToolCallFuse::new();
        let a = call("Read", serde_json::json!({"file_path":"a.rs"}));
        let b = call("Read", serde_json::json!({"file_path":"b.rs"}));
        let c = call("Read", serde_json::json!({"file_path":"c.rs"}));

        for tool_call in [&a, &b, &c, &a, &b, &c, &a, &b] {
            assert_eq!(fuse.inspect(tool_call), ToolFuseDecision::Allow);
        }
        assert!(matches!(
            fuse.inspect(&c),
            ToolFuseDecision::SoftBlock { .. }
        ));
    }

    #[test]
    fn normalizes_json_object_key_order_for_fingerprint() {
        let mut fuse = ToolCallFuse::new();
        let left = call("Read", serde_json::json!({"b":2,"a":1}));
        let right = call("Read", serde_json::json!({"a":1,"b":2}));

        assert_eq!(fuse.inspect(&left), ToolFuseDecision::Allow);
        assert_eq!(fuse.inspect(&right), ToolFuseDecision::Allow);
        assert!(matches!(
            fuse.inspect(&left),
            ToolFuseDecision::SoftBlock { .. }
        ));
    }
}
