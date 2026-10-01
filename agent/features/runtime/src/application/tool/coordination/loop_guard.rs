use crate::application::tool::agent::ToolCall;
use crate::application::tool::coordination::constants::{
    CONSECUTIVE_TOOL_CALL_HARD_LIMIT, CONSECUTIVE_TOOL_CALL_SOFT_LIMIT, MAX_INPUT_SUMMARY_CHARS,
    PERIOD_MAX_LEN, PERIOD_MIN_LEN, PERIOD_REPEAT_LIMIT, RECENT_TOOL_CALL_LIMIT,
    TOOL_FUSE_HARD_PAUSE_LIMIT,
};
use serde_json::Value;
use std::collections::VecDeque;

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
            let pattern = &forward[forward.len() - period_len..]; // allow unsafe_text_op: Vec slice
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
#[path = "loop_guard_tests.rs"]
mod tests;
