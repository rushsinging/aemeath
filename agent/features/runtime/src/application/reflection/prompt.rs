use share::i18n::runtime::reflection as t;

pub fn build_reflection_prompt(project_memory: &str, recent_summary: &str, lang: &str) -> String {
    t::reflection_prompt(lang, project_memory, recent_summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_reflection_prompt_contains_memory() {
        let prompt = build_reflection_prompt("- memory", "summary", "zh");

        assert!(prompt.contains("- memory"));
        assert!(prompt.contains("summary"));
    }

    #[test]
    fn test_build_reflection_prompt_requires_json() {
        let prompt_zh = build_reflection_prompt("", "", "zh");
        let prompt_en = build_reflection_prompt("", "", "en");

        assert!(prompt_zh.contains("只输出 JSON"));
        assert!(prompt_en.to_lowercase().contains("json only"));
        assert!(prompt_zh.contains("suggested_memories"));
        assert!(prompt_en.contains("suggested_memories"));
    }

    #[test]
    fn test_build_reflection_prompt_allows_empty_input() {
        let prompt_zh = build_reflection_prompt("", "", "zh");
        let prompt_en = build_reflection_prompt("", "", "en");

        assert!(prompt_zh.contains("# 当前项目记忆"));
        assert!(prompt_zh.contains("# 最近对话摘要"));
        assert!(prompt_en.contains("# Current project memory"));
        assert!(prompt_en.contains("# Recent conversation summary"));
    }
}
