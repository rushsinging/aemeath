//! scripts 组件契约测试：frontmatter 解析、校验、目录扫描。

use crate::adapters::scripts::{
    parse_script_frontmatter, scan_scripts_directory, validate_script, ScriptLang,
};
use std::path::PathBuf;

/// shebang + 注释块 frontmatter 解析。
#[test]
fn frontmatter_parsed_after_shebang() {
    let source = "#!/bin/bash\n# name: deploy\n# description: 部署脚本\nset -euo pipefail\n";
    let frontmatter = parse_script_frontmatter(source);
    assert_eq!(frontmatter.name.as_deref(), Some("deploy"));
    assert_eq!(frontmatter.description.as_deref(), Some("部署脚本"));
}

/// 无 shebang、空行穿插也不阻断首个注释块扫描。
#[test]
fn frontmatter_without_shebang_tolerates_blank_lines() {
    let source = "\n# description: 说明\n# name: x\nimport os\n";
    let frontmatter = parse_script_frontmatter(source);
    assert_eq!(frontmatter.name.as_deref(), Some("x"));
    assert_eq!(frontmatter.description.as_deref(), Some("说明"));
}

/// 正文第一行（非注释）终止扫描——正文中的伪键值不算 frontmatter。
#[test]
fn frontmatter_stops_at_first_code_line() {
    let source = "# description: 头部\nprint('# name: 正文里的不算')\n";
    let frontmatter = parse_script_frontmatter(source);
    assert_eq!(frontmatter.description.as_deref(), Some("头部"));
    assert!(frontmatter.name.is_none());
}

/// 语言推断：.sh / .py 识别，其余拒绝。
#[test]
fn script_lang_inferred_from_extension() {
    assert_eq!(
        ScriptLang::of_path(&PathBuf::from("a.sh")),
        Some(ScriptLang::Sh)
    );
    assert_eq!(
        ScriptLang::of_path(&PathBuf::from("a.py")),
        Some(ScriptLang::Py)
    );
    assert!(ScriptLang::of_path(&PathBuf::from("a.rb")).is_none());
}

/// 缺 description 必须被拒。
#[test]
fn missing_description_rejected() {
    let error = validate_script(&PathBuf::from("x.sh"), "#!/bin/bash\nset -e\necho ok\n")
        .expect_err("缺 description 必须拒绝");
    assert!(error.message().contains("description"));
}

/// 合法 sh 脚本通过（bash -n 可执行环境）。
#[test]
fn valid_sh_script_passes() {
    validate_script(
        &PathBuf::from("ok.sh"),
        "#!/bin/bash\n# description: 示例\necho hello\n",
    )
    .expect("合法 sh 必须通过");
}

/// sh 语法错误被 bash -n 检出。
#[test]
fn malformed_sh_rejected_by_bash_n() {
    let error = validate_script(
        &PathBuf::from("bad.sh"),
        "# description: 坏脚本\nif true; then\n  echo missing fi\n",
    )
    .expect_err("语法错误必须被拒");
    assert!(error.message().contains("bash -n"));
}

/// 合法 py 脚本通过（python3 可用时）。
#[test]
fn valid_py_script_passes() {
    validate_script(
        &PathBuf::from("ok.py"),
        "# description: 示例\nprint('hello')\n",
    )
    .expect("合法 py 必须通过");
}

/// 目录扫描：枚举 .sh/.py 按 stem，忽略其他类型与非文件。
#[test]
fn scan_enumerates_only_scripts_by_stem() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("alpha.sh"), "# description: a\n").expect("write");
    std::fs::write(temp.path().join("beta.py"), "# description: b\n").expect("write");
    std::fs::write(temp.path().join("notes.md"), "not a script").expect("write");
    std::fs::create_dir_all(temp.path().join("nested.sh")).expect("dir");

    let scripts = scan_scripts_directory(temp.path());
    let mut stems: Vec<String> = scripts.keys().cloned().collect();
    stems.sort();
    assert_eq!(stems, vec!["alpha".to_string(), "beta".to_string()]);
}

/// 不存在目录扫描返回空（NEVER 报错）。
#[test]
fn scan_missing_directory_yields_empty() {
    let scripts = scan_scripts_directory(&PathBuf::from("/nonexistent/scripts"));
    assert!(scripts.is_empty());
}
