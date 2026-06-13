use crate::tui::view_state::ViewModelDirty;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelChange {
    pub dirty: ViewModelDirty,
}

impl ModelChange {
    pub fn output_dirty() -> Self {
        let mut dirty = ViewModelDirty::default();
        dirty.mark_output();
        Self { dirty }
    }

    pub fn status_dirty() -> Self {
        let mut dirty = ViewModelDirty::default();
        dirty.mark_status();
        Self { dirty }
    }

    pub fn output_and_status_dirty() -> Self {
        let mut dirty = ViewModelDirty::default();
        dirty.mark_output();
        dirty.mark_status();
        Self { dirty }
    }

    pub fn dialog_dirty() -> Self {
        let mut dirty = ViewModelDirty::default();
        dirty.mark_dialog();
        Self { dirty }
    }
}

pub fn dirty_from_model_changes(changes: &[ModelChange]) -> ViewModelDirty {
    changes
        .iter()
        .fold(ViewModelDirty::default(), |mut dirty, change| {
            dirty.merge(&change.dirty);
            dirty
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dirty_from_model_changes_empty_is_clean() {
        assert_eq!(dirty_from_model_changes(&[]), ViewModelDirty::default());
    }

    #[test]
    fn test_dirty_from_model_changes_merges_output_and_status() {
        let dirty =
            dirty_from_model_changes(&[ModelChange::output_dirty(), ModelChange::status_dirty()]);
        assert!(dirty.output);
        assert!(dirty.status);
        assert!(!dirty.input);
        assert!(!dirty.dialog);
    }

    #[test]
    fn test_dirty_from_model_changes_preserves_dialog_dirty() {
        let dirty = dirty_from_model_changes(&[ModelChange::dialog_dirty()]);
        assert!(dirty.dialog);
        assert!(!dirty.output);
        assert!(!dirty.status);
    }
}
