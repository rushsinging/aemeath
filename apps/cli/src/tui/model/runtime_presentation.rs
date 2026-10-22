use crate::tui::view_model::status::ReasoningLevelView;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimePresentationIntent {
    ProviderModel {
        provider: Option<String>,
        model_id: Option<String>,
    },
    ContextSize(u64),
    /// Reasoning 呈现：开关与深度一起更新（#1616 状态栏直接显示 level）。
    Thinking {
        enabled: bool,
        level: ReasoningLevelView,
    },
    /// 当前模型 reasoning 能力上限（#2032）：bootstrap 与模型切换时更新，
    /// /think 对话框据此逐档渲染映射标注。
    ReasoningCapability {
        max_level: ReasoningLevelView,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimePresentationChange {
    ProviderModel {
        provider: Option<String>,
        model_id: Option<String>,
    },
    ContextSize(u64),
    Thinking {
        enabled: bool,
        level: ReasoningLevelView,
    },
    ReasoningCapability {
        max_level: ReasoningLevelView,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimePresentation {
    provider: Option<String>,
    model_id: Option<String>,
    context_size: u64,
    thinking: bool,
    reasoning_level: ReasoningLevelView,
    reasoning_max_level: ReasoningLevelView,
}

impl Default for RuntimePresentation {
    fn default() -> Self {
        Self {
            provider: None,
            model_id: None,
            context_size: 0,
            thinking: true,
            reasoning_level: ReasoningLevelView::High,
            // 能力上限收到 bootstrap/模型切换更新前按全档算（无映射标注）。
            reasoning_max_level: ReasoningLevelView::Max,
        }
    }
}

impl RuntimePresentation {
    pub(crate) fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    pub(crate) fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    pub(crate) fn context_size(&self) -> u64 {
        self.context_size
    }

    pub(crate) fn thinking(&self) -> bool {
        self.thinking
    }

    /// 当前呈现的 reasoning 深度（#1616 状态栏直接显示该值）。
    pub(crate) fn reasoning_level(&self) -> ReasoningLevelView {
        self.reasoning_level
    }

    /// 当前模型 reasoning 能力上限（#2032 对话框逐档映射标注）。
    pub(crate) fn reasoning_max_level(&self) -> ReasoningLevelView {
        self.reasoning_max_level
    }

    pub fn apply(&mut self, intent: RuntimePresentationIntent) -> RuntimePresentationChange {
        match intent {
            RuntimePresentationIntent::ProviderModel { provider, model_id } => {
                self.provider = provider.clone();
                self.model_id = model_id.clone();
                RuntimePresentationChange::ProviderModel { provider, model_id }
            }
            RuntimePresentationIntent::ContextSize(context_size) => {
                self.context_size = context_size;
                RuntimePresentationChange::ContextSize(context_size)
            }
            RuntimePresentationIntent::Thinking { enabled, level } => {
                self.thinking = enabled;
                self.reasoning_level = level;
                RuntimePresentationChange::Thinking { enabled, level }
            }
            RuntimePresentationIntent::ReasoningCapability { max_level } => {
                self.reasoning_max_level = max_level;
                RuntimePresentationChange::ReasoningCapability { max_level }
            }
        }
    }
}

#[cfg(test)]
#[path = "runtime_presentation_tests.rs"]
mod tests;
