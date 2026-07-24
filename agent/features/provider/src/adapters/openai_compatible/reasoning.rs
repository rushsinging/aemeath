use super::driver::ChatApiDriver;

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
            Self::ThinkingBudget(_tokens) => None,
            Self::Bool(_) => None,
        }
    }

    pub(super) fn for_scope(
        &self,
        level: crate::ReasoningLevel,
        driver: &dyn ChatApiDriver,
    ) -> ReasoningConfig {
        if matches!(level, crate::ReasoningLevel::Off) {
            return Self::Bool(false);
        }

        let effort = driver.clamp_effort(level.as_str()).to_string();
        match self {
            Self::Bool(_) => Self::Bool(true),
            Self::Object(value) => {
                let mut value = value.clone();
                if value.get("effort").is_some() {
                    value["effort"] = serde_json::Value::String(effort);
                } else if value.get("reasoning_effort").is_some() {
                    value["reasoning_effort"] = serde_json::Value::String(effort);
                } else {
                    value["effort"] = serde_json::Value::String(effort);
                }
                Self::Object(value)
            }
            // ThinkingBudget 与 effort 正交（#1393）：预算只表达启用 / 关闭，
            // 不参与 driver 的 effort clamp。保持原 `ThinkingBudget(tokens)`，
            // 由各 driver 在 `apply_reasoning_fields` 中按 toggle / budget 语义决定。
            Self::ThinkingBudget(_) => self.clone(),
        }
    }

    pub(super) fn from_scope(
        level: crate::ReasoningLevel,
        driver: &dyn ChatApiDriver,
    ) -> ReasoningConfig {
        if matches!(level, crate::ReasoningLevel::Off) {
            Self::Bool(false)
        } else {
            Self::Object(serde_json::json!({
                "effort": driver.clamp_effort(level.as_str())
            }))
        }
    }

    /// 返回经过 driver clamp 后的新配置。
    ///
    /// 对 `Object.effort` 字段调用 `driver.clamp_effort()` 做自适应降级；
    /// `ThinkingBudget` 与 effort 正交（#1393），保留原值不参与 clamp；
    /// `Bool` 不携带 effort 信息，直接返回自身。
    #[cfg(test)]
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
            Self::ThinkingBudget(_) => self.clone(),
            _ => self.clone(),
        }
    }
}
