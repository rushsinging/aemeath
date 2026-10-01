#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InputMode {
    #[default]
    Normal,
    Completion,
}

#[cfg(test)]
#[path = "mode_tests.rs"]
mod tests;
