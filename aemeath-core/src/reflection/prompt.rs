pub fn build_reflection_prompt(project_memory: &str, recent_summary: &str) -> String {
    format!(
        r#"你是 Aemeath 的 Reflection 引擎。请根据当前项目记忆和最近对话摘要，检查行为偏差、提出应写入的长期记忆、识别过时记忆。

要求：
- 只输出 JSON，不要输出 Markdown。
- suggested_memories[].category 只能是 fact、decision、preference、pattern、pitfall。
- outdated_memories 使用已有 memory id。
- 没有内容时输出空数组或 null。

JSON 格式：
{{
  "deviations": ["偏差描述"],
  "suggested_memories": [{{"category":"decision","content":"记忆内容","tags":["可选标签"]}}],
  "outdated_memories": ["memory-id"],
  "user_alert": "可选用户提示"
}}

# 当前项目记忆
{project_memory}

# 最近对话摘要
{recent_summary}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_reflection_prompt_contains_memory() {
        let prompt = build_reflection_prompt("- memory", "summary");

        assert!(prompt.contains("- memory"));
        assert!(prompt.contains("summary"));
    }

    #[test]
    fn test_build_reflection_prompt_requires_json() {
        let prompt = build_reflection_prompt("", "");

        assert!(prompt.contains("只输出 JSON"));
        assert!(prompt.contains("suggested_memories"));
    }

    #[test]
    fn test_build_reflection_prompt_allows_empty_input() {
        let prompt = build_reflection_prompt("", "");

        assert!(prompt.contains("# 当前项目记忆"));
        assert!(prompt.contains("# 最近对话摘要"));
    }
}
