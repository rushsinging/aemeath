use crate::tui::view_state::ViewModelDirty;

pub fn merge_dirty(target: &mut ViewModelDirty, source: ViewModelDirty) {
    target.merge(&source);
}

#[cfg(test)]
#[path = "dirty_tests.rs"]
mod tests;
