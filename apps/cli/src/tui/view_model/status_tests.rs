
// ── #2032：视图档位 clamp 映射 ──

#[test]
fn reasoning_level_view_clamps_to_model_maximum() {
    use super::ReasoningLevelView as Level;
    assert_eq!(Level::Max.clamped_to(Level::Medium), Level::Medium);
    assert_eq!(Level::High.clamped_to(Level::High), Level::High);
    assert_eq!(Level::Off.clamped_to(Level::Medium), Level::Off);
    assert_eq!(Level::Xhigh.clamped_to(Level::Max), Level::Xhigh);
}
