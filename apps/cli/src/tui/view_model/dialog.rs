use super::status::StatusSeverity;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogViewModel {
    pub kind: DialogKind,
    pub title: String,
    pub body: String,
    pub actions: Vec<DialogActionViewModel>,
    pub default_action: Option<String>,
    pub severity: StatusSeverity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogKind {
    Confirmation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogActionViewModel {
    pub id: String,
    pub label: String,
}
