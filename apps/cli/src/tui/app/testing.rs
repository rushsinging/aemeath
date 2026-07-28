#![cfg(test)]

mod effect_driver;
pub(crate) mod fixture;
mod harness;
pub(crate) mod input;
pub(crate) mod instrumented_backend;
mod screen;

pub(crate) use effect_driver::ExpectedEffect;
pub(crate) use harness::TuiScenarioHarness;
pub(crate) use screen::normalize_screen;
