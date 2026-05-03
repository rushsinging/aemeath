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
        AddResult::Added => "记忆已添加。".to_string(),
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
mod tests {
    use super::*;
    use crate::memory::{MemoryEntry, MemorySource};

    #[test]
    fn test_parse_layer_valid() {
        assert_eq!(parse_layer("global"), Some(MemoryLayer::Global));
        assert_eq!(parse_layer("p"), Some(MemoryLayer::Project));
    }

    #[test]
    fn test_parse_layer_invalid() {
        assert_eq!(parse_layer("session"), None);
        assert_eq!(parse_layer(""), None);
    }

    #[test]
    fn test_parse_category_valid() {
        assert_eq!(parse_category("decision"), Some(MemoryCategory::Decision));
        assert_eq!(parse_category("pitfall"), Some(MemoryCategory::Pitfall));
    }

    #[test]
    fn test_parse_category_invalid() {
        assert_eq!(parse_category("unknown"), None);
        assert_eq!(parse_category(""), None);
    }

    #[test]
    fn test_format_memory_list_empty() {
        assert_eq!(format_memory_list(&[]), "暂无记忆。");
    }

    #[test]
    fn test_format_memory_list_with_entry() {
        let entry = MemoryEntry::new(
            MemoryLayer::Project,
            MemoryCategory::Decision,
            "使用 JSON 存储",
            MemorySource::User,
        );
        let output = format_memory_list(&[entry]);

        assert!(output.contains("使用 JSON 存储"));
        assert!(output.contains("Project"));
        assert!(output.contains("Decision"));
    }
}
