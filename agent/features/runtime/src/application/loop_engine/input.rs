pub fn format_skill_request(request: &sdk::SkillRequest, language: &str) -> String {
    let zh = language.eq_ignore_ascii_case("zh") || language.starts_with("zh-");
    let mut lines = if zh {
        vec![
            "<skill-request>".to_string(),
            format!("用户请求使用 Skill：{}", request.skill),
        ]
    } else {
        vec![
            "<skill-request>".to_string(),
            format!("The user requests Skill: {}", request.skill),
        ]
    };
    if !request.arguments.is_empty() {
        lines.push(if zh {
            format!("参考参数：{}", request.arguments)
        } else {
            format!("Reference arguments: {}", request.arguments)
        });
    }
    lines.push(if zh {
        "请先调用 Skill 工具加载该 Skill，再结合参考参数理解并执行。".to_string()
    } else {
        "Call the Skill tool first to load this Skill, then interpret and execute it using the reference arguments.".to_string()
    });
    lines.push("</skill-request>".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod skill_request_tests {
    use super::format_skill_request;

    fn request(arguments: &str) -> sdk::SkillRequest {
        sdk::SkillRequest {
            input_id: sdk::InputId::new_v7(),
            skill: "release".to_string(),
            arguments: arguments.to_string(),
            raw_input: format!("/release {arguments}").trim().to_string(),
        }
    }

    #[test]
    fn formats_chinese_and_english_without_body_or_duplicate_arguments() {
        let zh = format_skill_request(&request("v1.2.3"), "zh");
        assert!(zh.contains("用户请求使用 Skill：release"));
        assert!(zh.contains("参考参数：v1.2.3"));
        assert_eq!(zh.matches("v1.2.3").count(), 1);
        assert!(!zh.contains("SKILL.md"));

        let en = format_skill_request(&request(""), "en");
        assert!(en.contains("The user requests Skill: release"));
        assert!(!en.contains("Reference arguments:"));
        assert!(en.contains("Call the Skill tool first"));
    }
}
