pub struct RunReport {
    pub name: String,
    pub first_exit: i32,
    pub retry_exits: Vec<i32>,
    pub passed: bool,
    pub classification: &'static str,
}

pub fn run_with_retry(name: &str, retries: usize, mut run: impl FnMut() -> i32) -> RunReport {
    let first_exit = run();
    let mut retry_exits = Vec::new();
    if first_exit != 0 {
        for _ in 0..retries {
            retry_exits.push(run());
        }
    }
    let classification = if first_exit == 0 {
        "passed"
    } else if retry_exits.contains(&0) {
        "flaky-suspect"
    } else {
        "reproducible-failure"
    };
    RunReport {
        name: name.into(),
        first_exit,
        retry_exits,
        passed: first_exit == 0,
        classification,
    }
}
