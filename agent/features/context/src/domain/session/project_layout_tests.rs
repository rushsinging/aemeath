use super::*;
use share::session_types::ProjectIdentityData;

fn git_identity(common_dir: &str) -> ProjectIdentityData {
    ProjectIdentityData {
        initial_cwd: format!("/tmp/worktree-{common_dir}"),
        git_common_dir: Some(common_dir.to_string()),
    }
}

fn plain_identity(cwd: &str) -> ProjectIdentityData {
    ProjectIdentityData {
        initial_cwd: cwd.to_string(),
        git_common_dir: None,
    }
}

#[test]
fn same_project_identity_maps_to_same_directory_segment() {
    let first = project_dir_segment(&git_identity("/repos/aemeath/.git"));
    let second = project_dir_segment(&git_identity("/repos/aemeath/.git"));
    assert_eq!(first.as_str(), second.as_str());
}

#[test]
fn worktrees_of_one_repository_share_segment() {
    // 同一仓库的不同 worktree：git_common_dir 相同，仅 initial_cwd 不同。
    let main_worktree = ProjectIdentityData {
        initial_cwd: "/repos/aemeath".to_string(),
        git_common_dir: Some("/repos/aemeath/.git".to_string()),
    };
    let feature_worktree = ProjectIdentityData {
        initial_cwd: "/repos/aemeath-worktrees/feature".to_string(),
        git_common_dir: Some("/repos/aemeath/.git".to_string()),
    };
    assert_eq!(
        project_dir_segment(&main_worktree).as_str(),
        project_dir_segment(&feature_worktree).as_str()
    );
}

#[test]
fn different_projects_map_to_different_segments() {
    let aemeath = project_dir_segment(&git_identity("/repos/aemeath/.git"));
    let other = project_dir_segment(&git_identity("/repos/other/.git"));
    assert_ne!(aemeath.as_str(), other.as_str());
}

#[test]
fn git_identity_and_plain_cwd_identity_never_collide_by_prefix() {
    let git_segment = project_dir_segment(&ProjectIdentityData {
        initial_cwd: "/repos/x".to_string(),
        git_common_dir: Some("/repos/x".to_string()),
    });
    let plain_segment = project_dir_segment(&plain_identity("/repos/x"));
    // 哈希输入带 `git:`/`cwd:` 前缀，两者派生自不同输入串。
    assert_ne!(git_segment.as_str(), plain_segment.as_str());
}

#[test]
fn segment_shape_is_sixteen_lowercase_hex_characters() {
    let segment = project_dir_segment(&plain_identity("/tmp/anywhere"));
    let value = segment.as_str();
    assert_eq!(value.len(), 16);
    assert!(
        value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()),
        "段必须是 16 个小写 hex 字符：{value}"
    );
}
