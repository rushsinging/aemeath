use super::*;

#[test]
fn pid_returns_process_id() {
    let p = pid();
    assert_eq!(p, std::process::id());
}
