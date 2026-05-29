//! 实时状态行（spinner + task 状态）视图模型。
//!
//! 纯数据：仅基本类型（String/u64/Option/Vec），不引用 model 内部类型或渲染库
//! （受 view_model 边界守卫约束）。phase 语义在 assembler 转换为 `phase_text`，
//! 此处只承载已格式化结果。

/// spinner 行的视图数据（动画 + 已转换的 phase 文案）。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpinnerLineView {
    /// 动画帧（来自 view_state，渲染层据此算 glyph 与微光）。
    pub frame: u64,
    /// 当前动词文本。
    pub verb: String,
    /// 已运行秒数（由 frame 推算，无需 Instant）。
    pub elapsed_secs: u64,
    /// 细分阶段文案（已由 phase 语义转换；None 表示无括号阶段）。
    pub phase_text: Option<String>,
}

/// 实时状态行整体视图：spinner（可缺省）+ 预格式化 task 行。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveStatusViewModel {
    /// spinner 行；None 表示 spinner 未激活。
    pub spinner: Option<SpinnerLineView>,
    /// task 状态预格式化显示行（透传自 Model 快照）。
    pub task_lines: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_live_status_view_model_default_is_empty() {
        let vm = LiveStatusViewModel::default();
        assert!(vm.spinner.is_none());
        assert!(vm.task_lines.is_empty());
    }

    #[test]
    fn test_spinner_line_view_holds_fields() {
        let view = SpinnerLineView {
            frame: 9,
            verb: "Thinking".to_string(),
            elapsed_secs: 1,
            phase_text: Some("Thinking...".to_string()),
        };
        assert_eq!(view.frame, 9);
        assert_eq!(view.phase_text.as_deref(), Some("Thinking..."));
    }

    #[test]
    fn test_live_status_view_model_equality() {
        let a = LiveStatusViewModel {
            spinner: Some(SpinnerLineView {
                frame: 1,
                verb: "Brewing".to_string(),
                elapsed_secs: 0,
                phase_text: None,
            }),
            task_lines: vec!["□ #1".to_string()],
        };
        let b = a.clone();
        assert_eq!(a, b);
        let c = LiveStatusViewModel::default();
        assert_ne!(a, c);
    }
}
