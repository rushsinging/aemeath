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
