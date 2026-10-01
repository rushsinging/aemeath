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
#[path = "change_tests.rs"]
mod tests;
