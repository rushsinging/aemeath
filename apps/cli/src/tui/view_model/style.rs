#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SemanticStyle {
    Normal,
    Muted,
    Running,
    Success,
    Error,
    Warning,
    /// 强调样式。
    Accent,
}
