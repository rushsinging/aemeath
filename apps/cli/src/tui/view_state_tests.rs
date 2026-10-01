use super::ViewModelDirty;

#[test]
fn test_view_model_dirty_tracks_and_clears_output() {
    let mut dirty = ViewModelDirty::default();
    assert!(!dirty.output);
    dirty.mark_output();
    assert!(dirty.output);
    dirty.clear_output();
    assert!(!dirty.output);
}
