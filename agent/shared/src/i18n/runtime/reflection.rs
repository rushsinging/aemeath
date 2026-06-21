//! Reflection 引擎文案（system prompt + 显示文本）。
//!
//! T8-1：reflection/prompt.rs 的 system prompt（原中文硬编码）+ format.rs 的
//! format_output 显示文本 + runner.rs 的 auto-apply 文案，全部双语化。

// ── reflection system prompt（给 LLM 的 JSON 提取指令）──────────

/// Reflection 引擎 system prompt 模板。
///
/// `{project_memory}` / `{recent_summary}` 为占位符，调用方 format! 替换。
/// 返回的模板需保证两种语言都包含 JSON 结构提示（deviations/suggested_memories/
/// outdated_memories/user_alert），否则 LLM 输出格式会错。
fn reflection_prompt_template(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"你是 Aemeath 的 Reflection 引擎。请根据当前项目记忆和最近对话摘要，检查行为偏差、提出应写入的长期记忆、识别过时记忆。

要求：
- 只输出 JSON，不要输出 Markdown。
- suggested_memories[].layer 只能是 project 或 global，默认优先使用 project。
- suggested_memories[].category 只能是 fact、decision、preference、pattern、pitfall。
- outdated_memories 使用已有 memory id。
- 没有内容时输出空数组或 null。

JSON 格式：
{{
    "deviations": ["偏差描述"],
    "suggested_memories": [{{"layer":"project","category":"decision","content":"记忆内容","tags":["可选标签"],"reason":"为什么建议添加"}}],
    "outdated_memories": ["memory-id"],
    "user_alert": "可选用户提示"
}}

# 当前项目记忆
{project_memory}

# 最近对话摘要
{recent_summary}"#
        }
        _ => {
            r#"You are the Aemeath Reflection engine. Based on the current project memory and recent conversation summary, detect behavioral deviations, suggest long-term memories to write, and identify outdated memories.

Requirements:
- Output JSON only, no Markdown.
- suggested_memories[].layer must be project or global; prefer project by default.
- suggested_memories[].category must be fact, decision, preference, pattern, or pitfall.
- outdated_memories uses existing memory ids.
- Output empty arrays or null when there is nothing.

JSON format:
{{
    "deviations": ["deviation description"],
    "suggested_memories": [{{"layer":"project","category":"decision","content":"memory content","tags":["optional tag"],"reason":"why this is suggested"}}],
    "outdated_memories": ["memory-id"],
    "user_alert": "optional user alert"
}}

# Current project memory
{project_memory}

# Recent conversation summary
{recent_summary}"#
        }
    }
}

/// 构建 Reflection system prompt（替换 project_memory / recent_summary 占位符）。
pub fn reflection_prompt(lang: &str, project_memory: &str, recent_summary: &str) -> String {
    reflection_prompt_template(lang)
        .replace("{project_memory}", project_memory)
        .replace("{recent_summary}", recent_summary)
}

// ── format_output 显示文本（偏差/记忆建议/过期记忆/用户提醒）──────────

pub fn section_title_reflection(lang: &str) -> &'static str {
    match lang {
        "zh" => "Reflection",
        _ => "Reflection",
    }
}

pub fn deviations_empty(lang: &str) -> &'static str {
    match lang {
        "zh" => "偏差：暂无明显偏差",
        _ => "Deviations: no significant deviations",
    }
}

pub fn deviations_header(lang: &str) -> &'static str {
    match lang {
        "zh" => "偏差：\n- ",
        _ => "Deviations:\n- ",
    }
}

pub fn suggestions_empty(lang: &str) -> &'static str {
    match lang {
        "zh" => "记忆建议：暂无建议",
        _ => "Memory suggestions: none",
    }
}

pub fn suggestions_header(lang: &str) -> &'static str {
    match lang {
        "zh" => "记忆建议：\n",
        _ => "Memory suggestions:\n",
    }
}

pub fn outdated_header(lang: &str) -> &'static str {
    match lang {
        "zh" => "过期记忆：",
        _ => "Outdated memories: ",
    }
}

pub fn user_alert_header(lang: &str) -> &'static str {
    match lang {
        "zh" => "用户提醒：",
        _ => "User alert: ",
    }
}

/// auto-apply 成功提示（runner.rs:87）。
pub fn auto_apply_summary(lang: &str, added: usize, outdated_marked: usize) -> String {
    match lang {
        "zh" => format!(
            "\n已自动应用 Reflection：新增/合并 {added} 条记忆，标记 {outdated_marked} 条过时记忆。"
        ),
        _ => format!(
            "\nReflection auto-applied: added/merged {added} memories, marked {outdated_marked} as outdated."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_prompt_bilingual_and_fallback() {
        let zh = reflection_prompt("zh", "mem", "sum");
        let en = reflection_prompt("en", "mem", "sum");
        assert!(zh.contains("你是 Aemeath 的 Reflection 引擎"));
        assert!(en.contains("You are the Aemeath Reflection engine"));
        assert_eq!(
            reflection_prompt("fr", "m", "s"),
            reflection_prompt("en", "m", "s")
        );
    }

    #[test]
    fn reflection_prompt_contains_json_schema_both_langs() {
        for lang in ["zh", "en"] {
            let p = reflection_prompt(lang, "", "");
            assert!(p.contains("deviations"), "{lang} must mention deviations");
            assert!(
                p.contains("suggested_memories"),
                "{lang} must mention suggested_memories"
            );
            assert!(
                p.contains("outdated_memories"),
                "{lang} must mention outdated_memories"
            );
            assert!(p.contains("user_alert"), "{lang} must mention user_alert");
        }
    }

    #[test]
    fn reflection_prompt_substitutes_placeholders() {
        let p = reflection_prompt("zh", "MY_MEMORY", "MY_SUMMARY");
        assert!(p.contains("MY_MEMORY"));
        assert!(p.contains("MY_SUMMARY"));
        // 占位符应被完全替换
        assert!(!p.contains("{project_memory}"));
        assert!(!p.contains("{recent_summary}"));
    }

    #[test]
    fn format_output_sections_bilingual() {
        assert!(deviations_empty("zh").contains("偏差"));
        assert!(deviations_empty("en").contains("Deviations"));
        assert!(suggestions_empty("zh").contains("记忆建议"));
        assert!(suggestions_empty("en").contains("Memory suggestions"));
    }

    #[test]
    fn auto_apply_summary_bilingual() {
        let zh = auto_apply_summary("zh", 3, 2);
        let en = auto_apply_summary("en", 3, 2);
        assert!(zh.contains("3") && zh.contains("2"));
        assert!(en.contains("3") && en.contains("2"));
        assert!(zh.contains("已自动应用"));
        assert!(en.contains("auto-applied"));
    }
}
