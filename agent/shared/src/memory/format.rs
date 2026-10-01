use super::{AddResult, MemoryCategory, MemoryEntry, MemoryLayer};

pub fn parse_layer(value: &str) -> Option<MemoryLayer> {
    match value.trim().to_lowercase().as_str() {
        "global" | "g" => Some(MemoryLayer::Global),
        "project" | "p" => Some(MemoryLayer::Project),
        _ => None,
    }
}

pub fn parse_category(value: &str) -> Option<MemoryCategory> {
    match value.trim().to_lowercase().as_str() {
        "fact" => Some(MemoryCategory::Fact),
        "decision" => Some(MemoryCategory::Decision),
        "preference" => Some(MemoryCategory::Preference),
        "pattern" => Some(MemoryCategory::Pattern),
        "pitfall" => Some(MemoryCategory::Pitfall),
        _ => None,
    }
}

pub fn format_memory_list(entries: &[MemoryEntry]) -> String {
    if entries.is_empty() {
        return "暂无记忆。".to_string();
    }

    let mut output = String::new();
    for entry in entries {
        output.push_str(&format!(
            "- {} [{} {:?}/{:?}] {}{}\n",
            short_id(&entry.id),
            if entry.pinned { "pinned" } else { "active" },
            entry.layer,
            entry.category,
            entry.content,
            format_tags(&entry.tags)
        ));
    }
    output
}

pub fn format_add_result(result: AddResult) -> String {
    match result {
        AddResult::Added { id } => {
            format!("记忆已添加。ID: {}", short_id(&id))
        }
        AddResult::Merged { existing_id } => {
            format!("已与相似记忆合并: {}", short_id(&existing_id))
        }
        AddResult::NeedsEviction { candidates } => {
            let mut output = String::from("记忆数量已达上限，请先归档候选记忆：\n");
            output.push_str(&format_memory_list(&candidates));
            output
        }
    }
}

pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn format_tags(tags: &[String]) -> String {
    if tags.is_empty() {
        String::new()
    } else {
        format!(" #{}", tags.join(" #"))
    }
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;
