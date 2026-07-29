use std::collections::HashMap;
use std::sync::LazyLock;

use super::traits::ToolDisplay;

pub struct ToolDisplayEntry {
    pub name: &'static str,
    pub display: fn() -> Box<dyn ToolDisplay>,
}

inventory::collect!(ToolDisplayEntry);

static TOOL_DISPLAYS: LazyLock<HashMap<&'static str, Box<dyn ToolDisplay>>> = LazyLock::new(|| {
    let mut map: HashMap<&'static str, Box<dyn ToolDisplay>> = HashMap::new();
    for entry in inventory::iter::<ToolDisplayEntry> {
        assert!(
            map.insert(entry.name, (entry.display)()).is_none(),
            "duplicate ToolDisplay registration: {}",
            entry.name
        );
    }
    map
});

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
