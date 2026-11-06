//! 落盘过期清理测试（roots / retention / now 全注入，零 env）。

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::*;

const EXPIRED_AGO: Duration = Duration::from_secs(40 * 86_400);

fn temp_root(tag: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("aemeath-disk-cleanup-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create temp root");
    root
}

/// 把树内全部**文件** mtime 回拨（目录自身 mtime 无法回拨，且判定
/// 口径为成员优先，见 [`super::dir_newest_mtime`]）。
fn backdate_tree(path: &Path, when: SystemTime) {
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                backdate_tree(&child, when);
            } else {
                set_mtime(&child, when);
            }
        }
    }
}

fn set_mtime(path: &Path, when: SystemTime) {
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .or_else(|_| fs::File::create(path))
        .expect("open for set_mtime");
    file.set_modified(when).expect("set_modified");
}

fn make_roots(root: &Path) -> DiskCleanupRoots {
    DiskCleanupRoots {
        session_datasets: root.join("state/session"),
        sessions: root.join("state/sessions"),
        tool_results: root.join("state/tool-results"),
        audit_usage: root.join("state/audit/usage"),
        tmp: root.join("state/tmp"),
        legacy_dirs: vec![
            root.join("agents/sessions"),
            root.join("agents/tool_outputs"),
        ],
        aemeath_home: root.join("home"),
    }
}

fn default_retention() -> share::config::cleanup::CleanupRetentionDaysConfig {
    share::config::cleanup::CleanupRetentionDaysConfig::default()
}

fn run(roots: &DiskCleanupRoots, dry_run: bool) -> DiskCleanupReport {
    run_disk_cleanup_with_roots(roots, &default_retention(), SystemTime::now(), dry_run)
}

fn face<'a>(report: &'a DiskCleanupReport, name: &str) -> &'a FaceCleanupReport {
    report
        .faces
        .iter()
        .find(|face| face.face == name)
        .unwrap_or_else(|| panic!("face {name} missing"))
}

/// 造一个过期 session 三件套（dataset + lock + background-process +
/// tool-results + ledger），并写入负载文件。
fn seed_expired_session(roots: &DiskCleanupRoots, id: &str) {
    let dataset = roots.session_datasets.join(format!("{id}.dataset"));
    fs::create_dir_all(&dataset).unwrap();
    fs::write(dataset.join("session.json"), b"payload").unwrap();
    fs::write(roots.sessions.join(format!("{id}.lock")), b"").unwrap();
    let bgp = roots.sessions.join(format!("{id}.background-process"));
    fs::create_dir_all(&bgp).unwrap();
    fs::write(bgp.join("bgp_1.log"), b"log").unwrap();
    let tr = roots.tool_results.join(id);
    fs::create_dir_all(&tr).unwrap();
    fs::write(tr.join("call_1"), b"tool result body").unwrap();
    fs::write(
        roots
            .session_datasets
            .join(format!("accepted-input-ledger-{id}.lock")),
        b"",
    )
    .unwrap();
    let when = SystemTime::now() - EXPIRED_AGO;
    backdate_tree(&dataset, when);
    backdate_tree(&bgp, when);
    backdate_tree(&tr, when);
    set_mtime(
        &roots
            .session_datasets
            .join(format!("accepted-input-ledger-{id}.lock")),
        when,
    );
    set_mtime(&roots.sessions.join(format!("{id}.lock")), when);
}

#[test]
fn expired_session_cascades_all_three_locations() {
    let root = temp_root("cascade");
    let roots = make_roots(&root);
    for dir in [
        &roots.session_datasets,
        &roots.sessions,
        &roots.tool_results,
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    seed_expired_session(&roots, "0199aaaa-1111-7000-8000-000000000001");

    let report = run(&roots, false);
    let sessions = face(&report, "sessions");
    assert!(!sessions.removed.is_empty());
    assert!(roots
        .session_datasets
        .join("0199aaaa-1111-7000-8000-000000000001.dataset")
        .read_dir()
        .is_err());
    assert!(!roots
        .sessions
        .join("0199aaaa-1111-7000-8000-000000000001.lock")
        .exists());
    assert!(!roots
        .sessions
        .join("0199aaaa-1111-7000-8000-000000000001.background-process")
        .exists());
    assert!(!roots
        .tool_results
        .join("0199aaaa-1111-7000-8000-000000000001")
        .exists());
    assert!(!roots
        .session_datasets
        .join("accepted-input-ledger-0199aaaa-1111-7000-8000-000000000001.lock")
        .exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn fresh_session_is_kept_intact() {
    let root = temp_root("fresh");
    let roots = make_roots(&root);
    for dir in [
        &roots.session_datasets,
        &roots.sessions,
        &roots.tool_results,
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    let id = "0199bbbb-2222-7000-8000-000000000002";
    let dataset = roots.session_datasets.join(format!("{id}.dataset"));
    fs::create_dir_all(&dataset).unwrap();
    fs::write(dataset.join("session.json"), b"payload").unwrap();
    fs::create_dir_all(roots.tool_results.join(id)).unwrap();

    let report = run(&roots, false);
    let sessions = face(&report, "sessions");
    assert!(
        sessions.removed.is_empty(),
        "removed: {:?}",
        sessions.removed
    );
    assert!(dataset.join("session.json").exists());
    assert!(roots.tool_results.join(id).is_dir());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn orphan_tool_results_swept_only_when_expired() {
    let root = temp_root("orphan");
    let roots = make_roots(&root);
    for dir in [
        &roots.session_datasets,
        &roots.sessions,
        &roots.tool_results,
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    let old_id = "0199cccc-3333-7000-8000-000000000003";
    let fresh_id = "0199dddd-4444-7000-8000-000000000004";
    for (id, backdate) in [(old_id, true), (fresh_id, false)] {
        let dir = roots.tool_results.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("call_1"), b"body").unwrap();
        if backdate {
            backdate_tree(&dir, SystemTime::now() - EXPIRED_AGO);
        }
    }

    run(&roots, false);
    assert!(
        !roots.tool_results.join(old_id).exists(),
        "expired orphan swept"
    );
    assert!(
        roots.tool_results.join(fresh_id).is_dir(),
        "fresh orphan kept"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn stale_revision_leftovers_follow_dataset_expiry() {
    let root = temp_root("stale");
    let roots = make_roots(&root);
    fs::create_dir_all(&roots.session_datasets).unwrap();
    let id = "0199eeee-5555-7000-8000-000000000005";
    // dataset 本体活跃，但 stale-revision 残留过期：锚点在 → 保留。
    let live = roots.session_datasets.join(format!("{id}.dataset"));
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("session.json"), b"payload").unwrap();
    let stale = roots
        .session_datasets
        .join(format!("{id}.dataset.stale-revision-29884"));
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("member.json"), b"stale payload").unwrap();
    backdate_tree(&stale, SystemTime::now() - EXPIRED_AGO);

    run(&roots, false);
    assert!(stale.is_dir(), "锚点活跃时 stale 残留保留");

    // 锚点删除后（模拟本体已清），残留按自身 mtime 兜底回收。
    fs::remove_dir_all(&live).unwrap();
    run(&roots, false);
    assert!(!stale.exists(), "孤儿 stale 残留回收");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn dry_run_removes_nothing_but_previews_bytes() {
    let root = temp_root("dryrun");
    let roots = make_roots(&root);
    for dir in [
        &roots.session_datasets,
        &roots.sessions,
        &roots.tool_results,
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    seed_expired_session(&roots, "0199ffff-6666-7000-8000-000000000006");

    let report = run(&roots, true);
    assert!(report.has_pending());
    assert!(report.total_reclaimed_bytes() > 0);
    assert!(roots
        .session_datasets
        .join("0199ffff-6666-7000-8000-000000000006.dataset")
        .is_dir());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn zero_days_disables_every_face() {
    let root = temp_root("disabled");
    let roots = make_roots(&root);
    fs::create_dir_all(&roots.session_datasets).unwrap();
    fs::create_dir_all(&roots.tmp).unwrap();
    fs::create_dir_all(&roots.audit_usage).unwrap();
    let retention = share::config::cleanup::CleanupRetentionDaysConfig {
        default: 0,
        ..default_retention()
    };
    let report = run_disk_cleanup_with_roots(&roots, &retention, SystemTime::now(), false);
    for face in report.faces.iter() {
        assert!(face.disabled, "face {} should be disabled", face.face);
        assert!(face.removed.is_empty());
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn age_faces_sweep_expired_entries() {
    let root = temp_root("agefaces");
    let roots = make_roots(&root);
    fs::create_dir_all(&roots.audit_usage).unwrap();
    fs::create_dir_all(&roots.tmp).unwrap();
    let legacy = roots.legacy_dirs[0].clone();
    fs::create_dir_all(legacy.join("hashdir")).unwrap();

    let old_audit = roots.audit_usage.join("0199.jsonl");
    fs::write(&old_audit, b"[]").unwrap();
    set_mtime(&old_audit, SystemTime::now() - EXPIRED_AGO);
    let fresh_audit = roots.audit_usage.join("0200.jsonl");
    fs::write(&fresh_audit, b"[]").unwrap();

    let old_tmp = roots.tmp.join("scratch");
    fs::create_dir_all(&old_tmp).unwrap();
    fs::write(old_tmp.join("part.bin"), b"data").unwrap();
    backdate_tree(&old_tmp, SystemTime::now() - EXPIRED_AGO);

    let old_legacy = legacy.join("hashdir");
    fs::create_dir_all(&old_legacy).unwrap();
    fs::write(old_legacy.join("legacy.lock"), b"").unwrap();
    backdate_tree(&old_legacy, SystemTime::now() - EXPIRED_AGO);

    let report = run(&roots, false);
    assert!(!old_audit.exists());
    assert!(fresh_audit.exists());
    assert!(!old_tmp.exists());
    assert!(!old_legacy.exists());
    assert!(face(&report, "audit").scanned >= 2);
    assert!(face(&report, "tmp").scanned >= 1);
    assert!(face(&report, "legacy").scanned >= 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn sweep_is_idempotent_on_second_run() {
    let root = temp_root("idempotent");
    let roots = make_roots(&root);
    for dir in [
        &roots.session_datasets,
        &roots.sessions,
        &roots.tool_results,
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    seed_expired_session(&roots, "01990000-7777-7000-8000-000000000007");
    run(&roots, false);
    let second = run(&roots, false);
    assert!(!second.has_pending(), "second run removes nothing");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn missing_roots_report_empty_not_error() {
    let root = temp_root("missing");
    let roots = make_roots(&root);
    let report = run(&roots, false);
    for face in report.faces.iter() {
        assert!(
            face.errors.is_empty(),
            "face {}: {:?}",
            face.face,
            face.errors
        );
        assert!(face.removed.is_empty());
    }
    assert!(!report.has_pending());
    let _ = fs::remove_dir_all(&root);
}
