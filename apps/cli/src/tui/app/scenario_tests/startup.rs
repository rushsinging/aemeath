use super::super::testing::TuiScenarioHarness;

#[test]
fn startup_renders_first_frame_with_fixed_backend() {
    let mut harness = TuiScenarioHarness::new(100, 30);

    harness.render();

    assert!(harness.screen().contains("Ready"));
    harness.assert_idle();
}
