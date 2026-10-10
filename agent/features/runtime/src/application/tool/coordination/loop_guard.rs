use crate::application::tool::agent::ToolCall;
use crate::application::tool::coordination::constants::{
    CONSECUTIVE_TOOL_CALL_HARD_LIMIT, CONSECUTIVE_TOOL_CALL_SOFT_LIMIT, MAX_INPUT_SUMMARY_CHARS,
    PERIOD_MAX_LEN, PERIOD_MIN_LEN, PERIOD_REPEAT_LIMIT, RECENT_TOOL_CALL_LIMIT,
    TOOL_FUSE_FAIL_LIMIT,
};
use sdk::ids::RunStepId;
use serde_json::Value;
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolFuseDecision {
    Allow,
    SoftBlock { reason: String },
    Fail { reason: String },
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct StepFingerprint {
    step_id: RunStepId,
    fingerprint: ToolCallFingerprint,
}

#[derive(Debug, Default)]
pub(crate) struct ToolCallFuse {
    recent: VecDeque<StepFingerprint>,
    blocked_count: usize,
}

impl ToolCallFuse {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 按 step 粒度计入：同一 `step_id` 内同一指纹无论出现几次只记 1 次。
    pub(crate) fn inspect(&mut self, step_id: &RunStepId, call: &ToolCall) -> ToolFuseDecision {
        let fingerprint = ToolCallFingerprint::from_call(call);
        if self
            .recent
            .iter()
            .any(|entry| &entry.step_id == step_id && entry.fingerprint == fingerprint)
        {
            return ToolFuseDecision::Allow;
        }

        self.recent.push_back(StepFingerprint {
            step_id: step_id.clone(),
            fingerprint: fingerprint.clone(),
        });
        while self.recent.len() > RECENT_TOOL_CALL_LIMIT {
            self.recent.pop_front();
        }

        let consecutive = self.consecutive_count(&fingerprint);
        let periodic = self.periodic_repeat();
        let soft_reason = if consecutive >= CONSECUTIVE_TOOL_CALL_SOFT_LIMIT {
            Some(format!(
                "repeated tool call detected: {} appeared {consecutive} consecutive steps",
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
            || self.blocked_count >= TOOL_FUSE_FAIL_LIMIT
        {
            ToolFuseDecision::Fail { reason }
        } else {
            ToolFuseDecision::SoftBlock { reason }
        }
    }

    fn consecutive_count(&self, fingerprint: &ToolCallFingerprint) -> usize {
        self.recent
            .iter()
            .rev()
            .take_while(|entry| &entry.fingerprint == fingerprint)
            .count()
    }

    fn periodic_repeat(&self) -> Option<(usize, usize, Vec<ToolCallFingerprint>)> {
        for period_len in PERIOD_MIN_LEN..=PERIOD_MAX_LEN {
            let required = period_len * PERIOD_REPEAT_LIMIT;
            if self.recent.len() < required {
                continue;
            }
            let forward = self
                .recent
                .iter()
                .map(|entry| entry.fingerprint.clone())
                .collect::<Vec<_>>();
            let pattern = &forward[forward.len() - period_len..]; // allow unsafe_text_op: Vec slice
            let start = forward.len() - required;
            if forward[start..] // allow unsafe_text_op: Vec slice
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
                    .map(|(key, value)| {
                        format!(
                            "{}:{}",
                            serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string()),
                            normalize_json(value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

#[cfg(test)]
#[path = "loop_guard_tests.rs"]
mod tests;
