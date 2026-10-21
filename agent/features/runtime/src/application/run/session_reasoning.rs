//! 会话级 reasoning 档位控制（#2032）。
//!
//! 单一状态源：`requested`（当前请求档位）+ `user_selected`（是否经 `/think`
//! 显式选择）。模型 entry 默认档与用户显式选择的竞争在此收敛：
//! `/model` 切换模型时仅当用户未显式选择才跟随新模型默认档。

use sdk::ThinkingRequest;
use share::reasoning::ReasoningLevel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SessionReasoningControl {
    requested: ReasoningLevel,
    user_selected: bool,
}

impl SessionReasoningControl {
    /// 会话初始：跟随模型 entry 默认档（未显式选择）。
    pub(crate) fn from_model_default(requested: ReasoningLevel) -> Self {
        Self {
            requested,
            user_selected: false,
        }
    }

    /// resume 恢复：session metadata 落盘的档位视为用户显式选择。
    pub(crate) fn from_user_selection(requested: ReasoningLevel) -> Self {
        Self {
            requested,
            user_selected: true,
        }
    }

    pub(crate) fn requested(&self) -> ReasoningLevel {
        self.requested
    }

    #[cfg(test)]
    pub(crate) fn user_selected(&self) -> bool {
        self.user_selected
    }

    /// 应用 `/think` 请求，返回生效的请求档位（provider 侧仍会 clamp 到能力上限）。
    pub(crate) fn apply_thinking_request(&mut self, request: ThinkingRequest) -> ReasoningLevel {
        let next = match request {
            ThinkingRequest::Level(level) => level,
            ThinkingRequest::On => ReasoningLevel::Medium,
            ThinkingRequest::Off => ReasoningLevel::Off,
            ThinkingRequest::Toggle => {
                if self.requested == ReasoningLevel::Off {
                    ReasoningLevel::Medium
                } else {
                    ReasoningLevel::Off
                }
            }
        };
        self.requested = next;
        self.user_selected = true;
        self.requested
    }

    /// `/model` 切换模型：用户显式选择保留原档位；否则跟随新模型默认档。
    pub(crate) fn on_model_switched(&mut self, model_default: ReasoningLevel) {
        if self.user_selected {
            return;
        }
        self.requested = model_default;
    }
}

#[cfg(test)]
#[path = "session_reasoning_tests.rs"]
mod tests;
