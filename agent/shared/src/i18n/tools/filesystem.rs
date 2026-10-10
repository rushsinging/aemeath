//! 文件系统工具文案（bash/grep/file_read/file_edit/file_write/glob 的 description）。
//!
//! 英文文案与各工具原 `description()` 完全一致；中文为等义翻译。

/// Bash description。
pub fn bash(lang: &str) -> &'static str {
    match lang {
        "zh" => "执行 bash 命令并返回输出。工作目录在多次调用间保持，但 shell 状态不保持——用 && 链接命令。",
        _ => "Executes a bash command and returns its output. The working directory persists between calls, but shell state does not — chain commands with `&&`.",
    }
}

/// Grep description。
pub fn grep(lang: &str) -> &'static str {
    match lang {
        "zh" => "使用 ripgrep 正则语法搜索文件内容，支持 glob 文件过滤。默认返回文件级索引（文件与匹配数），便于定位；需要查看具体匹配行时传 output_mode=\"content\"，或用 Read 读取目标文件。",
        _ => "Search file contents using ripgrep regex syntax, with glob file filters. Returns a file-level index (files with match counts) by default; pass output_mode=\"content\" to see matching lines, or use Read on the target files.",
    }
}

/// FileRead description。
pub fn file_read(lang: &str) -> &'static str {
    match lang {
        "zh" => "从本地文件系统读取文件。支持文本文件（带行号）和图片（PNG、JPG、GIF、WebP）。无法读取目录。",
        _ => "Reads a file from the local filesystem. Supports text files (with line numbers) and images (PNG, JPG, GIF, WebP). Cannot read directories.",
    }
}

/// FileEdit description。
pub fn file_edit(lang: &str) -> &'static str {
    match lang {
        "zh" => "在文件中执行精确字符串替换。必须先调用 Read。若 `old_string` 不唯一会失败——多处替换请用 `replace_all`。",
        _ => "Performs exact string replacements in files. Read must be called first. Fails if `old_string` is not unique — use `replace_all` for multiple occurrences.",
    }
}

/// FileWrite description。
pub fn file_write(lang: &str) -> &'static str {
    match lang {
        "zh" => "向本地文件系统写入文件。对已存在文件，必须先调用 Read。修改优先用 Edit；Write 用于新建文件或完全重写。",
        _ => "Writes a file to the local filesystem. For existing files, Read must be called first. Prefer Edit for modifications; use Write for new files or complete rewrites.",
    }
}

/// Glob description。
pub fn glob(lang: &str) -> &'static str {
    match lang {
        "zh" => "快速文件模式匹配工具。支持 glob 模式（如 \"**/*.rs\"）。按修改时间排序返回路径。",
        _ => "Fast file pattern matching tool. Supports glob patterns (e.g. \"**/*.rs\"). Returns paths sorted by modification time.",
    }
}

#[cfg(test)]
#[path = "filesystem_tests.rs"]
mod tests;
