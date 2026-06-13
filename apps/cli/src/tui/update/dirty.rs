use crate::tui::view_state::ViewModelDirty;

pub fn merge_dirty(target: &mut ViewModelDirty, source: ViewModelDirty) {
    target.merge(&source);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_dirty_preserves_existing_output() {
        let mut target = ViewModelDirty {
            output: true,
            ..Default::default()
        };
        merge_dirty(&mut target, ViewModelDirty::default());
        assert!(target.output);
    }

    #[test]
    fn test_merge_dirty_adds_status() {
        let mut target = ViewModelDirty::default();
        let source = ViewModelDirty {
            status: true,
            ..Default::default()
        };
        merge_dirty(&mut target, source);
        assert!(target.status);
    }

    #[test]
    fn test_merge_dirty_adds_input_and_dialog() {
        let mut target = ViewModelDirty::default();
        let source = ViewModelDirty {
            input: true,
            dialog: true,
            ..Default::default()
        };
        merge_dirty(&mut target, source);
        assert!(target.input && target.dialog);
    }
}
