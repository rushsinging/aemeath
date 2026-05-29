#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SemanticStyle {
    Normal,
    Muted,
    Running,
    Success,
    Error,
    Warning,
    /// 预留：强调样式（后续渲染管线 S 任务接线）。
    #[allow(dead_code)]
    Accent,
}
