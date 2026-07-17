use std::path::PathBuf;

pub(super) const CWD_MARKER: &str = "__AEMEATH_CWD__=";

pub(super) fn split_stdout_and_cwd(stdout: &str) -> (String, Option<PathBuf>) {
    let Some(pos) = stdout.rfind(CWD_MARKER) else {
        return (stdout.to_string(), None);
    };
    let before_marker = &stdout[..pos];
    let after_marker = &stdout[pos + CWD_MARKER.len()..];
    let Some(first_line_end) = after_marker.find('\n') else {
        return (stdout.to_string(), None);
    };
    let cwd = after_marker[..first_line_end].trim();
    if cwd.is_empty() {
        return (stdout.to_string(), None);
    }

    let visible_stdout = before_marker.trim_end_matches('\n').to_string();
    (visible_stdout, Some(PathBuf::from(cwd)))
}
