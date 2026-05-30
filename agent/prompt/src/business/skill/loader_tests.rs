use super::*;
use std::io::Write;

#[test]
fn test_load_skills_from_subdir() {
    let base = std::env::temp_dir().join("aemeath_test_skill_3");
    let sub = base.join("review");
    std::fs::create_dir_all(&sub).unwrap();

    // Direct .md file
    let direct = base.join("hello.md");
    let mut f = std::fs::File::create(&direct).unwrap();
    write!(f, "---\ndescription: hello skill\n---\nhello").unwrap();

    // Sub-dir .md file
    let sub_file = sub.join("SKILL.md");
    let mut f = std::fs::File::create(&sub_file).unwrap();
    write!(f, "---\ndescription: review skill\n---\nreview").unwrap();

    let skills = load_skills_from_dir(&base);
    assert_eq!(
        skills.len(),
        2,
        "should load both direct and sub-dir skills"
    );
    assert!(skills.iter().any(|s| s.name == "hello"), "direct skill");
    assert!(skills.iter().any(|s| s.name == "review"), "sub-dir skill");

    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn test_load_skills_nested_with_namespace() {
    // Simulate: ~/.agents/skills/superpowers/skills/brainstorming/SKILL.md
    let base = std::env::temp_dir().join("aemeath_test_skill_ns");
    let deep = base
        .join("superpowers")
        .join("skills")
        .join("brainstorming");
    std::fs::create_dir_all(&deep).unwrap();

    let skill_file = deep.join("SKILL.md");
    let mut f = std::fs::File::create(&skill_file).unwrap();
    write!(
        f,
        "---\nname: brainstorming\ndescription: test\n---\nbrainstorm content"
    )
    .unwrap();

    let skills = load_skills_from_dir(&base);
    assert_eq!(skills.len(), 1, "should find nested skill");
    assert_eq!(skills[0].name, "superpowers:brainstorming");
    assert!(
        skills[0].aliases.contains(&"brainstorming".to_string()),
        "original name should be an alias"
    );

    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn test_load_skills_ignores_non_skills_dirs() {
    // Non-"skills" dirs at nested levels should be skipped
    let base = std::env::temp_dir().join("aemeath_test_skill_ignore");
    let pkg = base.join("superpowers");
    let skills_dir = pkg.join("skills").join("my-skill");
    let agents_dir = pkg.join("agents");
    let github_dir = pkg.join(".github").join("ISSUE_TEMPLATE");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::create_dir_all(&github_dir).unwrap();

    // Real skill
    let skill_file = skills_dir.join("SKILL.md");
    let mut f = std::fs::File::create(&skill_file).unwrap();
    write!(f, "---\nname: my-skill\ndescription: real\n---\ncontent").unwrap();

    // Agent file (should be ignored — agents/ is not "skills")
    let agent_file = agents_dir.join("code-reviewer.md");
    let mut f = std::fs::File::create(&agent_file).unwrap();
    write!(
        f,
        "---\nname: code-reviewer\ndescription: agent\n---\nagent content"
    )
    .unwrap();

    // GitHub issue template (should be ignored)
    let issue_file = github_dir.join("bug_report.md");
    let mut f = std::fs::File::create(&issue_file).unwrap();
    write!(
        f,
        "---\nname: bug_report\ndescription: template\n---\ntemplate"
    )
    .unwrap();

    let skills = load_skills_from_dir(&base);
    assert_eq!(
        skills.len(),
        1,
        "should only find the skill, not agent/template"
    );
    assert_eq!(skills[0].name, "superpowers:my-skill");

    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn test_load_skills_no_namespace_for_regular_dirs() {
    // Regular skill directories (no `skills/` child) should NOT get namespace prefix
    let base = std::env::temp_dir().join("aemeath_test_no_ns");
    let sub = base.join("review");
    std::fs::create_dir_all(&sub).unwrap();

    let sub_file = sub.join("SKILL.md");
    let mut f = std::fs::File::create(&sub_file).unwrap();
    write!(f, "---\ndescription: review skill\n---\nreview content").unwrap();

    let skills = load_skills_from_dir(&base);
    assert_eq!(skills.len(), 1);
    assert_eq!(
        skills[0].name, "review",
        "no namespace prefix for regular dirs"
    );

    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn test_load_all_skills_prefers_project_claude_skills() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_skill_claude_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let claude_skills = base.join(".claude").join("skills");
    let agents_skills = base.join(".agents").join("skills");
    std::fs::create_dir_all(&claude_skills).unwrap();
    std::fs::create_dir_all(&agents_skills).unwrap();
    let mut claude_file = std::fs::File::create(claude_skills.join("demo.md")).unwrap();
    write!(
        claude_file,
        "---\nname: demo\ndescription: claude\n---\nclaude skill"
    )
    .unwrap();
    let mut agents_file = std::fs::File::create(agents_skills.join("demo.md")).unwrap();
    write!(
        agents_file,
        "---\nname: demo\ndescription: agents\n---\nagents skill"
    )
    .unwrap();

    let skills = load_all_skills(&base, &[]);

    assert!(skills.contains_key("demo"));
    assert_eq!(skills["demo"].source_path, claude_skills.join("demo.md"));

    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn test_load_all_skills_falls_back_to_project_agents_skills() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_skill_agents_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project_skills = base.join(".agents").join("skills");
    std::fs::create_dir_all(&project_skills).unwrap();
    let mut file = std::fs::File::create(project_skills.join("demo.md")).unwrap();
    write!(
        file,
        "---\nname: demo\ndescription: demo\n---\nproject skill"
    )
    .unwrap();

    let skills = load_all_skills(&base, &[]);

    assert!(skills.contains_key("demo"));
    assert_eq!(skills["demo"].source_path, project_skills.join("demo.md"));

    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn test_load_all_skills_includes_builtin_commit_skill() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_builtin_commit_skill_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).unwrap();

    let skills = load_all_skills(&base, &[]);
    let commit = skills
        .get("commit")
        .expect("commit skill should be built in");
    let content = super::super::read_skill_content(commit);

    assert_eq!(commit.name, "commit");
    assert!(commit.aliases.contains(&"git-commit".to_string()));
    assert!(commit.description.contains("git commit"));
    assert!(content.contains("git status --short --branch"));
    assert!(content.contains("git log --format=%B --grep='Co-Authored-By' -n 20"));
    assert!(content.contains("Co-Authored-By: Aemeath"));
    assert!(content.contains("git commit"));

    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn test_load_all_skills_scans_project_global_and_extra_dirs() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_skill_all_dirs_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project_skills = base.join(".agents").join("skills");
    let extra_skills = base.join("extra-skills");
    std::fs::create_dir_all(&project_skills).unwrap();
    std::fs::create_dir_all(&extra_skills).unwrap();

    let mut project_file = std::fs::File::create(project_skills.join("project.md")).unwrap();
    write!(
        project_file,
        "---\nname: project-only\ndescription: project\n---\nproject skill"
    )
    .unwrap();
    let mut extra_file = std::fs::File::create(extra_skills.join("extra.md")).unwrap();
    write!(
        extra_file,
        "---\nname: extra-only\ndescription: extra\n---\nextra skill"
    )
    .unwrap();

    let skills = load_all_skills(&base, std::slice::from_ref(&extra_skills));

    assert!(skills.contains_key("project-only"));
    assert!(skills.contains_key("extra-only"));
    assert_eq!(
        skills["project-only"].source_path,
        project_skills.join("project.md")
    );
    assert_eq!(
        skills["extra-only"].source_path,
        extra_skills.join("extra.md")
    );

    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn test_global_skills_dir_points_to_home_agents_skills() {
    let expected = dirs::home_dir()
        .map(|home| home.join(".agents").join("skills"))
        .unwrap_or_else(|| std::path::PathBuf::from(".agents").join("skills"));

    assert_eq!(global_skills_dir(), expected);
}

#[test]
fn test_project_skill_can_override_builtin_commit_skill() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_builtin_commit_override_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project_skills = base.join(".agents").join("skills");
    std::fs::create_dir_all(&project_skills).unwrap();
    let mut file = std::fs::File::create(project_skills.join("commit.md")).unwrap();
    write!(
        file,
        "---\nname: commit\ndescription: project commit\n---\nproject commit skill"
    )
    .unwrap();

    let skills = load_all_skills(&base, &[]);

    assert!(skills.contains_key("commit"));
    assert_eq!(
        skills["commit"].source_path,
        project_skills.join("commit.md")
    );

    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn test_load_all_skills_does_not_auto_migrate_project_aemeath_skills() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_skill_no_auto_migration_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let old_skills = base.join(".aemeath").join("skills");
    std::fs::create_dir_all(&old_skills).unwrap();
    let mut file = std::fs::File::create(old_skills.join("legacy.md")).unwrap();
    write!(
        file,
        "---\nname: legacy\ndescription: legacy\n---\nlegacy skill"
    )
    .unwrap();

    let skills = load_all_skills(&base, &[]);
    let new_skills = base.join(".agents").join("skills");

    assert!(!new_skills.exists());
    assert!(!skills.contains_key("legacy"));

    std::fs::remove_dir_all(base).unwrap();
}
