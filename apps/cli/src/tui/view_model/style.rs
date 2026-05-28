#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticStyle {
    Normal,
    Muted,
    Running,
    Success,
    Error,
    Warning,
    Accent,
}
