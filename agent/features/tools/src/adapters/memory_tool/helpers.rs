use memory::api::{MemoryCategory, MemoryLayer};
use serde_json::Value;

pub(super) const MAX_CONTENT_CHARS: usize = 500;
pub(super) const MAX_TAGS: usize = 10;
pub(super) const MAX_TAG_CHARS: usize = 32;

pub(super) fn required_string<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("缺少必需参数: {key}"))
}

pub(super) fn optional_layer(input: &Value) -> Result<Option<MemoryLayer>, String> {
    match input.get("layer").and_then(|value| value.as_str()) {
        Some(layer) => match layer {
            "global" => Ok(Some(MemoryLayer::Global)),
            "project" => Ok(Some(MemoryLayer::Project)),
            _ => Err(format!("无效 memory layer: {layer}")),
        },
        None => Ok(None),
    }
}

pub(super) fn optional_category(input: &Value) -> Result<Option<MemoryCategory>, String> {
    match input.get("category").and_then(|value| value.as_str()) {
        Some(category) => match category {
            "fact" => Ok(Some(MemoryCategory::Fact)),
            "decision" => Ok(Some(MemoryCategory::Decision)),
            "preference" => Ok(Some(MemoryCategory::Preference)),
            "pattern" => Ok(Some(MemoryCategory::Pattern)),
            "pitfall" => Ok(Some(MemoryCategory::Pitfall)),
            _ => Err(format!("无效 memory category: {category}")),
        },
        None => Ok(None),
    }
}

pub(super) fn parse_tags(input: &Value) -> Result<Vec<String>, String> {
    let Some(tags) = input.get("tags").and_then(|value| value.as_array()) else {
        return Ok(Vec::new());
    };
    if tags.len() > MAX_TAGS {
        return Err(format!("tags 不能超过 {MAX_TAGS} 个"));
    }

    let mut parsed = Vec::new();
    for tag in tags {
        let Some(tag) = tag.as_str() else {
            return Err("tag 必须是字符串".to_string());
        };
        let tag = tag.trim();
        if tag.is_empty() {
            return Err("tag 不能为空".to_string());
        }
        if tag.chars().count() > MAX_TAG_CHARS {
            return Err(format!("tag 不能超过 {MAX_TAG_CHARS} 字符"));
        }
        parsed.push(tag.to_string());
    }
    parsed.sort();
    parsed.dedup();
    Ok(parsed)
}

pub(super) fn validate_content(content: &str) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err("memory content 不能为空".to_string());
    }
    if content.chars().count() > MAX_CONTENT_CHARS {
        return Err(format!("memory content 不能超过 {MAX_CONTENT_CHARS} 字符"));
    }
    Ok(())
}
