use std::fs;
use std::path::Path;

fn rust_files_under(path: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).expect("read test directory") {
            let entry = entry.expect("read directory entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files
}

fn production_source(source: &str) -> String {
    let mut output = String::new();
    let mut skip_test_module = false;
    let mut brace_depth = 0usize;

    for line in source.lines() {
        if line.trim() == "#[cfg(test)]" {
            skip_test_module = true;
            continue;
        }
        if skip_test_module {
            let opens = line.matches('{').count();
            let closes = line.matches('}').count();
            if opens > 0 || brace_depth > 0 {
                brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);
                if brace_depth == 0 {
                    skip_test_module = false;
                }
            }
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }

    output
}

#[test]
fn test_tui_facade_reexports_only_app_entrypoint() {
    let tui_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui.rs");
    let source = fs::read_to_string(&tui_root).expect("read tui facade");

    assert!(
        source.contains("pub use self::app::App;"),
        "tui facade must publish the App entrypoint"
    );
    let reexports_render = source.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("pub use ") && trimmed.contains("render")
    });
    assert!(
        !reexports_render,
        "tui facade must not re-export render widgets; they are internal implementation details"
    );
}
#[test]
fn test_adapter_and_view_assembler_production_do_not_depend_on_render_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let checked_dirs = [root.join("adapter"), root.join("view_assembler")];

    for dir in checked_dirs {
        for file in rust_files_under(&dir) {
            if file
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("test"))
            {
                continue;
            }
            let source = production_source(&fs::read_to_string(&file).expect("read rust source"));
            assert!(
                !source.contains("crate::tui::render::"),
                "{} production code must not depend on render modules",
                file.display()
            );
        }
    }
}
