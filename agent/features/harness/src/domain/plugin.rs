//! plugin 组件（c8）domain：声明式 seam 规则模型与引擎。
//!
//! plugin 是**声明式数据**（markdown + 规则表），NEVER 动态代码——
//! 对齐架构守卫边界（NEVER 引入任意 exec）。首批 seam：
//! `trim_output`（按工具裁剪超长输出）、`error_hint`（按错误签名
//! 追加 hint）。生效粒度 run 边界：run 开始时快照规则集，loop 内
//! NEVER 中途换（#2030 契约）。

use crate::constants::ERROR_DOMAIN;
use crate::domain::errors::HarnessError;
use serde::{Deserialize, Serialize};

/// 单条 plugin seam 规则（数据驱动，v1 两形态）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "seam", rename_all = "snake_case")]
pub enum PluginRule {
    /// 裁剪指定工具的超长输出（字符预算，带截断标记）。
    TrimOutput {
        /// 目标工具名（精确匹配；空 = 全部工具）。
        tool: String,
        /// 输出字符上限。
        max_chars: usize,
    },
    /// 结果为错误且文本命中签名时，尾部追加 hint 行。
    ErrorHint {
        /// 目标工具名（精确匹配；空 = 全部工具）。
        tool: String,
        /// 错误签名（子串匹配，大小写敏感）。
        pattern: String,
        /// 追加的提示行。
        hint: String,
    },
}

impl PluginRule {
    fn matches_tool(&self, tool_name: &str) -> bool {
        let rule_tool = match self {
            Self::TrimOutput { tool, .. } => tool,
            Self::ErrorHint { tool, .. } => tool,
        };
        rule_tool.is_empty() || rule_tool == tool_name
    }
}

/// seam 应用结果（供 TUI 渲染与审计）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeamOutcome {
    pub rule_index: usize,
    pub plugin_name: String,
    pub applied: SeamEffect,
}

/// 单次 seam 生效形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeamEffect {
    Trimmed { original_chars: usize },
    Hinted,
}

/// seam 引擎：规则集按序应用，首个 trim 生效后仍允许后续 hint 追加
/// （trim 改文本、hint 追加行——两者正交）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginSeamEngine {
    /// 规则来源 plugin 名（与规则一一对应）。
    rules: Vec<(String, PluginRule)>,
}

impl PluginSeamEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个 plugin 的规则集。
    pub fn register(&mut self, plugin_name: &str, rules: &[PluginRule]) {
        for rule in rules {
            self.rules.push((plugin_name.to_string(), rule.clone()));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// 对单次工具结果应用全部匹配规则。
    pub fn apply(&self, tool_name: &str, text: &str, is_error: bool) -> (String, Vec<SeamOutcome>) {
        let mut output = text.to_string();
        let mut outcomes = Vec::new();
        for (index, (plugin_name, rule)) in self.rules.iter().enumerate() {
            if !rule.matches_tool(tool_name) {
                continue;
            }
            match rule {
                PluginRule::TrimOutput { max_chars, .. } => {
                    let original_chars = output.chars().count();
                    if original_chars > *max_chars {
                        let mut trimmed: String = output.chars().take(*max_chars).collect();
                        trimmed.push_str("…（plugin seam 截断）");
                        output = trimmed;
                        outcomes.push(SeamOutcome {
                            rule_index: index,
                            plugin_name: plugin_name.clone(),
                            applied: SeamEffect::Trimmed { original_chars },
                        });
                    }
                }
                PluginRule::ErrorHint { pattern, hint, .. } => {
                    if is_error && output.contains(pattern.as_str()) {
                        output.push('\n');
                        output.push_str(hint);
                        outcomes.push(SeamOutcome {
                            rule_index: index,
                            plugin_name: plugin_name.clone(),
                            applied: SeamEffect::Hinted,
                        });
                    }
                }
            }
        }
        (output, outcomes)
    }
}

/// 校验单条规则参数（v1：上限与签名非空）。
pub fn validate_plugin_rule(rule: &PluginRule) -> Result<(), HarnessError> {
    match rule {
        PluginRule::TrimOutput { max_chars, .. } => {
            if *max_chars == 0 {
                return Err(HarnessError::invalid(
                    ERROR_DOMAIN,
                    "trim_output 规则 max_chars MUST > 0",
                ));
            }
        }
        PluginRule::ErrorHint { pattern, hint, .. } => {
            if pattern.trim().is_empty() || hint.trim().is_empty() {
                return Err(HarnessError::invalid(
                    ERROR_DOMAIN,
                    "error_hint 规则 pattern 与 hint MUST 非空",
                ));
            }
        }
    }
    Ok(())
}
