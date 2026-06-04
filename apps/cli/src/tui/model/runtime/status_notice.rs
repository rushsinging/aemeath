#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusNoticeKind {
    #[default]
    Normal,
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
mod tests {
    use super::*;

    #[test]
    fn test_status_notice_default_is_ready_normal() {
        let notice = StatusNotice::default();

        assert_eq!(notice.text, "Ready");
        assert_eq!(notice.kind, StatusNoticeKind::Normal);
    }

    #[test]
    fn test_status_notice_success_sets_kind_and_text() {
        let notice = StatusNotice::success("Copied");

        assert_eq!(notice.text, "Copied");
        assert_eq!(notice.kind, StatusNoticeKind::Success);
    }

    #[test]
    fn test_status_notice_warning_sets_kind_and_text() {
        let notice = StatusNotice::warning("Interrupted");

        assert_eq!(notice.text, "Interrupted");
        assert_eq!(notice.kind, StatusNoticeKind::Warning);
    }
}
