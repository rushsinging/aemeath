use super::style::SemanticStyle;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatusLineViewModel {
    pub left: Vec<StatusSegment>,
    pub center: Vec<StatusSegment>,
    pub right: Vec<StatusSegment>,
    pub severity: StatusSeverity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusSegment {
    pub key: String,
    pub text: String,
    pub style: SemanticStyle,
    pub priority: u8,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusSeverity {
    #[default]
    Normal,
    Info,
    Warning,
    Error,
}
