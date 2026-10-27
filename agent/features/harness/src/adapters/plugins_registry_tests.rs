//! plugins registry 契约测试（#2030）。

use crate::adapters::plugins_registry::{parse_plugin_rules, PluginsDirectoryRegistry};
use crate::application::ComponentRegistry;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::plugin::PluginRule;

fn plugin_id(name: &str) -> ComponentId {
    ComponentId {
        kind: ComponentKind::Plugin,
        name: ComponentName::new(name),
    }
}

const VALID_PLUGIN: &str = "---\nname: output-guard\ndescription: 输出防护\n---\n## seam: trim_output\ntool: Bash\nmax_chars: 2000\n\n## seam: error_hint\ntool: Bash\npattern: \"exit code 1\"\nhint: \"先查 stderr\"\n";

/// 规则解析：分节 + 参数体 → PluginRule 集。
#[test]
fn parse_rules_from_plugin_body() {
    let rules = parse_plugin_rules(
        "## seam: trim_output\ntool: Bash\nmax_chars: 100\n\n## seam: error_hint\ntool: \"\"\npattern: \"not found\"\nhint: 换 Glob\n",
    )
    .expect("解析");
    assert_eq!(
        rules,
        vec![
            PluginRule::TrimOutput {
                tool: "Bash".to_string(),
                max_chars: 100,
            },
            PluginRule::ErrorHint {
                tool: String::new(),
                pattern: "not found".to_string(),
                hint: "换 Glob".to_string(),
            },
        ]
    );
}

/// 未知 seam 类型 / 空参数体 / 非法参数拒绝。
#[test]
fn parse_rejects_unknown_seam_and_bad_params() {
    assert!(parse_plugin_rules("## seam: run_code\nx: 1\n").is_err());
    assert!(parse_plugin_rules("## seam: trim_output\n").is_err());
    assert!(parse_plugin_rules("## seam: trim_output\nmax_chars: 0\n").is_err());
}

/// registry list 枚举含 plugin.md 的目录。
#[test]
fn list_enumerates_plugin_directories() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("guard")).expect("dir");
    std::fs::write(temp.path().join("guard/plugin.md"), VALID_PLUGIN).expect("write");
    std::fs::create_dir_all(temp.path().join("empty")).expect("dir");

    let registry = PluginsDirectoryRegistry::new(temp.path());
    assert_eq!(registry.list(), vec![plugin_id("guard")]);
}

/// read 返回 frontmatter + 规则正文。
#[test]
fn read_extracts_frontmatter_and_body() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("guard")).expect("dir");
    std::fs::write(temp.path().join("guard/plugin.md"), VALID_PLUGIN).expect("write");

    let registry = PluginsDirectoryRegistry::new(temp.path());
    let content = registry.read(&plugin_id("guard")).expect("read");
    assert_eq!(content.frontmatter.expect("fm")["description"], "输出防护");
    assert!(content.body.contains("## seam: trim_output"));
}

/// write 校验先行：合法落盘、非法（缺 description）拒绝。
#[test]
fn write_validates_before_persist() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut registry = PluginsDirectoryRegistry::new(temp.path());
    let valid = ComponentContent {
        frontmatter: None,
        body: VALID_PLUGIN.to_string(),
    };
    registry.write(&plugin_id("fresh"), &valid).expect("write");
    assert!(temp.path().join("fresh/plugin.md").exists());

    let broken = ComponentContent {
        frontmatter: None,
        body: "正文无规则无frontmatter差异但经解析—缺规则节也可?不,校验需description在fm"
            .to_string(),
    };
    assert!(registry.write(&plugin_id("broken"), &broken).is_err());
    assert!(!temp.path().join("broken").exists());
}
