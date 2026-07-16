#![cfg(test)]

mod effect_driver;
mod fixture;
mod harness;
pub(crate) mod input;
mod screen;

pub(crate) use effect_driver::ExpectedEffect;
pub(crate) use harness::TuiScenarioHarness;
pub(crate) use screen::normalize_screen;
