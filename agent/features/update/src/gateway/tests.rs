use std::io::Write;

use crate::contract::GitHubRelease;

use super::archive::extract_binary_from_tar_gz;
use super::checksum::{parse_checksums, sha256_hex};
use super::platform::{download_url, platform_target};
use super::version::{build_version_check, current_version, strip_v_prefix};

#[test]
fn test_strip_v_prefix_with_prefix() {
    assert_eq!(strip_v_prefix("v0.9.0"), "0.9.0");
}

#[test]
fn test_strip_v_prefix_without_prefix() {
    assert_eq!(strip_v_prefix("0.9.0"), "0.9.0");
}

#[test]
fn test_build_version_check_newer() {
    // 模拟一个比当前版本新的 release
    let release = GitHubRelease {
        tag_name: format!("v{}.{}.{}", current_version().major + 1, 0, 0),
        html_url: "https://example.com".into(),
        body: Some("notes".into()),
        prerelease: false,
    };
    let check = build_version_check(&release).unwrap();
    assert!(check.is_update_available);
    assert_eq!(check.release_url, "https://example.com");
}

#[test]
fn test_build_version_check_same_version() {
    let release = GitHubRelease {
        tag_name: format!("v{}", current_version()),
        html_url: "https://example.com".into(),
        body: None,
        prerelease: false,
    };
    let check = build_version_check(&release).unwrap();
    assert!(!check.is_update_available);
    assert!(check.release_notes.is_none());
}

#[test]
fn test_build_version_check_older_version() {
    // 模拟一个比当前版本旧的 release
    let cur = current_version();
    // 构造明确比 current 旧的版本；current == 0.0.0（无 tag fallback）时无更旧版本，跳过
    let older_tag = if cur.patch > 0 {
        format!("v{}.{}.{}", cur.major, cur.minor, cur.patch - 1)
    } else if cur.minor > 0 {
        format!("v{}.{}.0", cur.major, cur.minor - 1)
    } else if cur.major > 0 {
        format!("v{}.0.0", cur.major - 1)
    } else {
        return;
    };
    let release = GitHubRelease {
        tag_name: older_tag,
        html_url: "https://example.com".into(),
        body: None,
        prerelease: false,
    };
    let check = build_version_check(&release).unwrap();
    assert!(!check.is_update_available);
}

// ── perform_update 辅助函数测试 ──────────────────────────────────

#[test]
fn test_platform_target_matches_current() {
    let target = platform_target();
    assert!(
        target.is_some(),
        "当前平台 (os={}, arch={}) 应受支持",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

#[test]
fn test_download_url_format() {
    let url = download_url("0.9.0", "aemeath-0.9.0-aarch64-apple-darwin.tar.gz");
    assert_eq!(
        url,
        "https://github.com/rushsinging/aemeath/releases/download/v0.9.0/aemeath-0.9.0-aarch64-apple-darwin.tar.gz"
    );
}

#[test]
fn test_parse_checksums_found() {
    let content = "\
a1b2c3d4e5f6  aemeath-0.9.0-aarch64-apple-darwin.tar.gz\n\
f7e8d9c0b1a2  aemeath-0.9.0-x86_64-apple-darwin.tar.gz\n";
    let hash = parse_checksums(content, "aemeath-0.9.0-aarch64-apple-darwin.tar.gz");
    assert_eq!(hash.as_deref(), Some("a1b2c3d4e5f6"));
}

#[test]
fn test_parse_checksums_not_found() {
    let content = "a1b2c3d4e5f6  other-file.tar.gz\n";
    let hash = parse_checksums(content, "aemeath-0.9.0-aarch64-apple-darwin.tar.gz");
    assert!(hash.is_none());
}

#[test]
fn test_parse_checksums_case_insensitive_hash() {
    let content = "A1B2C3D4E5F6  aemeath-0.9.0-aarch64-apple-darwin.tar.gz\n";
    let hash = parse_checksums(content, "aemeath-0.9.0-aarch64-apple-darwin.tar.gz");
    assert_eq!(hash.as_deref(), Some("a1b2c3d4e5f6"));
}

#[test]
fn test_parse_checksums_empty_and_whitespace() {
    let content = "\n  \n  \n";
    let hash = parse_checksums(content, "any.tar.gz");
    assert!(hash.is_none());
}

#[test]
fn test_sha256_hex_known_value() {
    // SHA256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
    let hash = sha256_hex(b"hello");
    assert_eq!(
        hash,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn test_sha256_hex_empty() {
    // SHA256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    let hash = sha256_hex(b"");
    assert_eq!(
        hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn test_extract_binary_from_tar_gz() {
    // 构造一个最小 tar.gz 归档：目录 + 文件 aemeath
    let mut builder = tar::Builder::new(Vec::new());
    let dir_path = "aemeath-0.9.0-x86_64-apple-darwin/";
    let mut dir_header = tar::Header::new_gnu();
    dir_header.set_path(dir_path).unwrap();
    dir_header.set_size(0);
    dir_header.set_entry_type(tar::EntryType::Directory);
    dir_header.set_mode(0o755);
    dir_header.set_cksum();
    builder.append(&dir_header, std::io::empty()).unwrap();

    let bin_data = b"#!/bin/sh\necho fake binary\n";
    let mut file_header = tar::Header::new_gnu();
    file_header.set_path(format!("{dir_path}aemeath")).unwrap();
    file_header.set_size(bin_data.len() as u64);
    file_header.set_entry_type(tar::EntryType::Regular);
    file_header.set_mode(0o755);
    file_header.set_cksum();
    builder.append(&file_header, &bin_data[..]).unwrap();

    let tar_bytes = builder.into_inner().unwrap();
    let mut gz_buf = Vec::new();
    {
        let mut encoder =
            flate2::write::GzEncoder::new(&mut gz_buf, flate2::Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        encoder.finish().unwrap();
    }

    let extracted = extract_binary_from_tar_gz(&gz_buf).unwrap();
    assert_eq!(extracted, bin_data);
}

#[test]
fn test_extract_binary_from_tar_gz_not_found() {
    let mut builder = tar::Builder::new(Vec::new());
    let data = b"some content";
    let mut header = tar::Header::new_gnu();
    header.set_path("other.txt").unwrap();
    header.set_size(data.len() as u64);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, &data[..]).unwrap();

    let tar_bytes = builder.into_inner().unwrap();
    let mut gz_buf = Vec::new();
    {
        let mut encoder =
            flate2::write::GzEncoder::new(&mut gz_buf, flate2::Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        encoder.finish().unwrap();
    }

    let result = extract_binary_from_tar_gz(&gz_buf);
    assert!(result.is_err());
}
