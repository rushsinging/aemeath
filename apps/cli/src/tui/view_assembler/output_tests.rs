include!("output_tests/common_tests.rs");
include!("output_tests/tool_result_tests.rs");
include!("output_tests/bench_tests.rs");

#[path = "output_tests/edit_diff_performance_tests.rs"]
mod edit_diff_performance;
#[path = "output_tests/retained_state_performance_tests.rs"]
mod retained_state_performance;
