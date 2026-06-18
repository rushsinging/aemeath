//! 从 git tag 注入版本号到编译期常量。
//!
//! 用 `git describe --tags --abbrev=0` 取最近 tag，strip `v` 前缀后通过
//! `cargo:rustc-env=AEMEATH_VERSION=<x.y.z>` 注入。
//! 运行时通过 `share::VERSION`（= `option_env!("AEMEATH_VERSION").unwrap_or(CARGO_PKG_VERSION)`）
//! 访问，取不到时 fallback 到 `Cargo.toml` 的 `version`（占位符 `0.0.0`）。

use std::process::Command;

fn main() {
    // tag 变化时重新构建
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-changed=.git/HEAD");

    // git describe --tags --abbrev=0：取最近的 reachable tag
    let output = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .output();

    let tag = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            // 无 tag 或 git 不可用：不设 AEMEATH_VERSION，编译时 fallback 到 CARGO_PKG_VERSION
            return;
        }
    };

    // strip 前缀 'v' 或 'V'
    let version = tag.strip_prefix(['v', 'V']).unwrap_or(&tag);

    println!("cargo:rustc-env=AEMEATH_VERSION={version}");
}
