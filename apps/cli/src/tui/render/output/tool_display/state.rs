//! 状态容器（#1146 placement 归位）。
use super::traits::ToolDisplay;
use super::ToolDisplayEntry;
use std::collections::HashMap;
use std::sync::LazyLock;

pub(crate) static TOOL_DISPLAYS: LazyLock<HashMap<&'static str, Box<dyn ToolDisplay>>> =
    LazyLock::new(|| {
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
