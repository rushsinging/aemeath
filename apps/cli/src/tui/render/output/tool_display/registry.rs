use super::state::TOOL_DISPLAYS;

use super::traits::ToolDisplay;

pub struct ToolDisplayEntry {
    pub name: &'static str,
    pub display: fn() -> Box<dyn ToolDisplay>,
}

inventory::collect!(ToolDisplayEntry);

#[cfg(test)]
pub(crate) fn registration_count(name: &str) -> usize {
    inventory::iter::<ToolDisplayEntry>
        .into_iter()
        .filter(|entry| entry.name == name)
        .count()
}

pub(crate) fn lookup_display(name: &str) -> Option<&'static dyn ToolDisplay> {
    TOOL_DISPLAYS.get(name).map(|display| display.as_ref())
}
