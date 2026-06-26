use super::driver::ChatApiDriver;
use super::effort_from_thinking_tokens;

#[derive(Debug, Clone, PartialEq)]
pub enum ReasoningConfig {
    Bool(bool),
    Object(serde_json::Value),
    ThinkingBudget(u32),
}

impl ReasoningConfig {
    pub(super) fn as_effort(&self) -> Option<String> {
        match self {
            Self::Object(value) => value
                .get("effort")
                .or_else(|| value.get("reasoning_effort"))
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned),
            Self::ThinkingBudget(tokens) => Some(effort_from_thinking_tokens(*tokens).to_string()),
            Self::Bool(_) => None,
        }
    }

    /// 返回经过 driver clamp 后的新配置。
    ///
    /// 对 Object 中的 effort 字段和 ThinkingBudget 推导出的 effort，
    /// 调用 `driver.clamp_effort()` 做自适应降级。
    pub(super) fn clamped(&self, driver: &dyn ChatApiDriver) -> ReasoningConfig {
        match self {
            Self::Object(obj) => {
                if let Some(effort) = obj.get("effort").and_then(|v| v.as_str()) {
                    let clamped = driver.clamp_effort(effort);
                    if clamped != effort {
                        let mut new_obj = obj.clone();
                        new_obj["effort"] = serde_json::Value::String(clamped.to_string());
                        Self::Object(new_obj)
                    } else {
                        self.clone()
                    }
                } else {
                    self.clone()
                }
            }
            Self::ThinkingBudget(_) => {
                if let Some(effort) = self.as_effort() {
                    let clamped = driver.clamp_effort(&effort);
                    Self::Object(serde_json::json!({"effort": clamped}))
                } else {
                    self.clone()
                }
            }
            _ => self.clone(),
        }
    }
}
