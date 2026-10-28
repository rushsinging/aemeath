use super::*;

fn texts(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn extracts_typed_ids_and_paths_once() {
    let input = texts(&[
        "见 agent/features/context/src/domain.rs 与 run_082gKx9yRcG",
        "再提一次 run_082gKx9yRcG 和 agent/features/context/src/domain.rs:42",
    ]);
    let found = extract_identifiers(&input);
    assert!(found.iter().any(|item| item == "run_082gKx9yRcG"));
    assert!(found
        .iter()
        .any(|item| item == "agent/features/context/src/domain.rs"));
    assert_eq!(
        found
            .iter()
            .filter(|item| *item == "run_082gKx9yRcG")
            .count(),
        1
    );
}

#[test]
fn drops_eval_noise_paths_and_short_names() {
    let input = texts(&["eval/compact/harness/metrics.py", "a.rs", "plain words"]);
    assert!(extract_identifiers(&input).is_empty());
}

#[test]
fn ranks_higher_frequency_first_and_applies_limit() {
    let input = texts(&[
        "run_abc123 run_abc123 mem_def456",
        "run_abc123 mem_def456",
        "stp_ghi789",
    ]);
    assert_eq!(
        rank_identifiers(&input, 2),
        vec!["run_abc123", "mem_def456"]
    );
}

#[test]
fn ties_are_broken_alphabetically_for_stability() {
    let input = texts(&["beta.rs alpha.rs"]);
    assert_eq!(rank_identifiers(&input, 10), vec!["alpha.rs", "beta.rs"]);
}
