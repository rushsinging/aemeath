#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusNoticeKind {
    #[default]
    Normal,
    Running,
    Success,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusNotice {
    pub text: String,
    pub kind: StatusNoticeKind,
}

impl Default for StatusNotice {
    fn default() -> Self {
        Self {
            text: "Ready".to_string(),
            kind: StatusNoticeKind::Normal,
        }
    }
}

impl StatusNotice {
    pub fn ready() -> Self {
        Self::default()
    }

    pub fn normal(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: StatusNoticeKind::Normal,
        }
    }

    pub fn running(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: StatusNoticeKind::Running,
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: StatusNoticeKind::Success,
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: StatusNoticeKind::Warning,
        }
    }
}

#[cfg(test)]
#[path = "status_notice_tests.rs"]
mod tests;
