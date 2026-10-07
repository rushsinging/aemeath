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
    /// Runtime 发布的当前 Run 总耗时。
    pub elapsed_secs: u64,
    /// Runtime 发布的当前状态阶段耗时。
    pub phase_elapsed_secs: Option<u64>,
    /// 细分阶段文案（已由 phase 语义转换；None 表示无括号阶段）。
    pub phase_text: Option<String>,
    /// 最多一条通过稳定性门槛的用户可见 Activity 摘要。
    pub detail_text: Option<String>,
    /// #252：后台任务活动数（0 = 不显示；spinner 尾部 ⛙N）。
    pub background_tasks_active: usize,
}

/// Compact 进度视图（spinner 行内嵌渲染用）。
///
/// `ratio_millis` 为 ratio * 1000（0–1000），避免 f64 破坏 `Eq` 约束。
/// `stage` / `current` / `total` 供渲染层构造阶段文案与百分比。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactProgressView {
    pub ratio_millis: u32,
    pub stage: String,
    pub current: Option<u32>,
    pub total: Option<u32>,
}

/// 实时状态行整体视图：spinner（可缺省）+ 排队输入预览行 + 预格式化 task 行。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveStatusViewModel {
    /// spinner 行；None 表示 spinner 未激活。
    pub spinner: Option<SpinnerLineView>,
    /// 排队输入预览行（每条一行，显示在 spinner 上方）。
    pub queued_lines: Vec<String>,
    /// task 状态预格式化显示行（透传自 Model 快照）。
    pub task_lines: Vec<String>,
    /// compact 进度（spinner 行内嵌）；None 表示未在 compact 中。
    pub compact_progress: Option<CompactProgressView>,
}

#[cfg(test)]
#[path = "live_status_tests.rs"]
mod tests;
