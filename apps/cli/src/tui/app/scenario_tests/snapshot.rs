use super::super::testing::normalize_screen;

#[test]
fn screen_normalization_preserves_layout_and_trims_only_trailing_space() {
    let normalized = normalize_screen("A  \n  B \n\n");
    assert_eq!(normalized, "A\n  B");
}
