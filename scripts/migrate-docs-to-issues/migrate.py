#!/usr/bin/env python3
"""
migrate.py — 将 docs/bug 与 docs/feature 下的所有条目批量迁移到 GitHub Issues。

模式：
  --dry-run（默认）：仅生成 migration-map.json 与 draft/<kind>_<id>.md 草稿，
                    不实际创建 issue
  --apply：调用 `gh issue create` 批量创建

幂等：在 issue body 顶部写入 `<!-- Migrated from: <source> -->` 溯源注释；
      远端 issue 通过 `gh issue list --label migrated-from:docs --json body`
      拉取已有溯源注释，跳过已迁移条目。

输出：
  migration-map.json  完整条目 → issue 编号映射（apply 完成后含真实 issue 编号）
  draft/*.md          每个条目一份 issue body 草稿，便于人工复核
  draft/summary.json  草稿统计

不修改任何 docs/ 下的源文件。
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Optional

REPO_ROOT = Path(__file__).resolve().parents[2]
DOCS_BUG = REPO_ROOT / "docs" / "bug"
DOCS_FEATURE = REPO_ROOT / "docs" / "feature"
SPECS_DIR = DOCS_FEATURE / "specs"
SCRIPT_DIR = Path(__file__).resolve().parent
DRAFT_DIR = SCRIPT_DIR / "draft"
MAP_PATH = SCRIPT_DIR / "migration-map.json"

KIND_BUG = "bug"
KIND_FEATURE = "feature"

# 溯源注释：写入 issue body 顶部，--apply 阶段用于幂等去重
SOURCE_MARKER = re.compile(r"^<!--\s*Migrated from:\s*(?P<src>[^\s]+)\s*-->\s*$", re.M)

# 优先级字段归一化
PRIORITY_MAP = {
    "高": "high", "中": "medium", "低": "low",
    "high": "high", "medium": "medium", "low": "low",
    "-": None, "": None,
}

# 表格行解析（bug active.md）
# | 112 | TUI tool call spinner 有状态但输出区不显示 tool card | 中 | 待确认 | 待用户确认 | 2026-06 | runtime tool 事件携带 chat/turn context，TUI 按上下文绑定 conversation |
ROW_RE = re.compile(
    r"^\|\s*(?P<id>\d+)\s*\|\s*(?P<title>[^|]+?)\s*\|\s*(?P<priority>[^|]*?)\s*\|\s*(?P<state>[^|]*?)\s*\|\s*(?P<rest>.*?)\s*\|\s*$"
)

# 详情段头：### #<id> <标题> 或 ## #<id> <标题>
DETAIL_HEADER_RE = re.compile(r"^#{2,3}\s+#(?P<id>\d+)\s+(?P<title>.+?)\s*$")

# 归档文件标题（兼容多种格式）：
#   `# Bug #3 优化 tool call TUI 显示`        (Bug/Feature + 空格)
#   `# Bug #30: 对话过程中 input queue ...`    (Bug/Feature + 英文冒号)
#   `# Bug #78：input area 粘贴后...`           (Bug/Feature + 中文冒号)
#   `# Bug 61: Diff 渲染行号...`                (无 # 包 id)
#   `# #19 config model 支持 zhipu api 类型`  (无 Bug/Feature 前缀)
ARCHIVE_TITLE_RE = re.compile(
    r"^#\s*(?:(?:(?P<kind>Bug|Feature))\s*)?(?:\#)?(?P<id>\d+)\s*[:：]?\s*(?P<title>.+?)\s*$"
)

# 归档文件名：`{id:03d}-{slug}.md`（id 三位补零），文件名 id 为权威
ARCHIVE_FILENAME_RE = re.compile(r"^(?P<id>\d{3})-(?P<slug>.+)\.md$")


@dataclass
class Entry:
    """单条待迁移条目。"""
    kind: str  # 'bug' or 'feature'
    id: int
    title: str
    priority: Optional[str] = None
    state: Optional[str] = None
    source: str = ""  # 相对仓库根的源文件路径
    body: str = ""  # issue 正文
    status_in_body: Optional[str] = None  # body 中显式「**状态**：xxx」
    issue_number: Optional[int] = None  # 实际 issue 编号（仅 --apply 后填）

    def to_dict(self) -> dict:
        return asdict(self)

    def labels(self) -> list[str]:
        labels = [f"kind:{self.kind}"]
        if self.priority:
            labels.append(f"priority:{self.priority}")
        labels.append("migrated-from:docs")
        return labels

    def draft_filename(self) -> str:
        return f"{self.kind}_{self.id:03d}.md"


def normalize_priority(raw: str) -> Optional[str]:
    raw = (raw or "").strip()
    return PRIORITY_MAP.get(raw, None)


def split_md_table_row(line: str) -> Optional[dict]:
    """解析一行 markdown 表格。返回 id/title/priority/state/rest 字段；非数据行返回 None。"""
    m = ROW_RE.match(line)
    if not m:
        return None
    return {
        "id": int(m.group("id")),
        "title": m.group("title").strip(),
        "priority": normalize_priority(m.group("priority")),
        "state": m.group("state").strip() or None,
        "rest": m.group("rest").strip(),
    }


def extract_status_from_body(body: str) -> Optional[str]:
    """从 body 中提取 `**状态**：xxx` 字段。"""
    m = re.search(r"^\*\*状态\*\*[：:]\s*(?P<v>.+?)\s*$", body, re.M)
    return m.group("v").strip() if m else None


def wrap_body(source_path: str, original_body: str) -> str:
    """在 body 顶部插入溯源注释。"""
    marker = f"<!-- Migrated from: {source_path} -->\n"
    return marker + original_body.rstrip() + "\n"


# ---------------------------------------------------------------------------
# Parsers
# ---------------------------------------------------------------------------


def parse_active_file(kind: str, path: Path) -> list[Entry]:
    """解析 docs/{bug,feature}/active.md：表格行 + 详情段。"""
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    rel = str(path.relative_to(REPO_ROOT))

    # 1. 找表格区域
    table_rows: dict[int, dict] = {}
    in_table = False
    for line in lines:
        if re.match(r"^\|\s*#\s*\|", line):
            in_table = True
            continue
        if in_table:
            if re.match(r"^\|[\s:|-]+\|$", line):
                continue  # 分隔行
            if not line.startswith("|"):
                in_table = False
                continue
            row = split_md_table_row(line)
            if row:
                table_rows[row["id"]] = row

    # 2. 找详情段（header 自身也加入 body，保留原始 md 结构）
    detail_blocks: dict[int, tuple[str, str]] = {}  # id -> (title, body)
    current_id: Optional[int] = None
    current_title: Optional[str] = None
    current_buf: list[str] = []
    for line in lines:
        m = DETAIL_HEADER_RE.match(line)
        if m:
            if current_id is not None:
                detail_blocks[current_id] = (current_title or "", "\n".join(current_buf).strip("\n"))
            current_id = int(m.group("id"))
            current_title = m.group("title").strip()
            current_buf = [line]  # header 自身也加入 body
        else:
            if current_id is not None:
                current_buf.append(line)
    if current_id is not None:
        detail_blocks[current_id] = (current_title or "", "\n".join(current_buf).strip("\n"))

    # 3. 合并：表格行 + 详情段
    # source 拼接 `#<id>` 后缀，让 active.md 内多条不同 id 各自独立溯源
    entries: list[Entry] = []
    for entry_id, row in sorted(table_rows.items()):
        detail = detail_blocks.get(entry_id)
        entry_source = f"{rel}#{entry_id}"
        if detail:
            detail_title, detail_body = detail
            title = detail_title or row["title"]
            body = wrap_body(entry_source, detail_body)
            status = extract_status_from_body(detail_body) or row["state"]
        else:
            title = row["title"]
            body = wrap_body(entry_source, f"**状态**：{row['state'] or '未知'}\n\n**目标**：{row['rest']}\n")
            status = row["state"]
        entries.append(Entry(
            kind=kind,
            id=entry_id,
            title=title,
            priority=row["priority"],
            state=status,
            source=entry_source,
            body=body,
        ))
    return entries


def parse_archive_index(kind: str, path: Path) -> dict[int, dict]:
    """解析 docs/{bug,feature}/archive.md：仅索引。返回 id -> {title, slug}。"""
    text = path.read_text(encoding="utf-8")
    rows: dict[int, dict] = {}
    for line in text.splitlines():
        m = re.match(
            r"^\|\s*(?P<id>\d+)\s*\|\s*(?P<title>[^|]+?)\s*\|\s*\[archived/(?P<slug>[^\]]+)\]\([^)]+\)\s*\|\s*$",
            line,
        )
        if m:
            rows[int(m.group("id"))] = {
                "title": m.group("title").strip(),
                "slug": m.group("slug").strip(),
            }
    return rows


def parse_archived_file(kind: str, path: Path) -> Optional[Entry]:
    """解析 docs/{bug,feature}/archived/<id>-<slug>.md：单条详情。

    优先用文件名 id 作为权威；标题从首行正则解析；
    如首行无法解析，回退到文件名 slug 转 title。
    """
    text = path.read_text(encoding="utf-8")
    rel = str(path.relative_to(REPO_ROOT))

    # 1. 文件名解析（权威 id）
    fn_m = ARCHIVE_FILENAME_RE.match(path.name)
    if not fn_m:
        print(f"warn: {rel} 文件名不符合 {{id:03d}}-slug.md 格式", file=sys.stderr)
        return None
    entry_id = int(fn_m.group("id"))
    slug_title = fn_m.group("slug").replace("-", " ").strip()

    # 2. 标题解析（从首行）
    first_line = text.splitlines()[0] if text else ""
    title = None
    m = ARCHIVE_TITLE_RE.match(first_line)
    if m and int(m.group("id")) == entry_id:
        title = m.group("title").strip()
    if not title:
        title = slug_title

    # 3. 优先级与状态
    priority = None
    pm = re.search(r"^\*\*优先级\*\*[：:]\s*(?P<v>.+?)\s*$", text, re.M)
    if pm:
        priority = normalize_priority(pm.group("v"))
    status = extract_status_from_body(text)

    body = wrap_body(rel, text.rstrip())
    return Entry(
        kind=kind,
        id=entry_id,
        title=title,
        priority=priority,
        state=status,
        source=rel,
        body=body,
    )


def parse_archived_dir(kind: str, archived_dir: Path) -> list[Entry]:
    if not archived_dir.is_dir():
        return []
    entries: list[Entry] = []
    for p in sorted(archived_dir.iterdir()):
        if p.suffix == ".md" and p.is_file():
            entry = parse_archived_file(kind, p)
            if entry:
                entries.append(entry)
    return entries


def collect_all_entries() -> list[Entry]:
    """收集所有待迁移条目（去重：active + archived 同 id 时取 archived，保留所有来源）。"""
    all_entries: list[Entry] = []

    if DOCS_BUG.is_dir():
        active = DOCS_BUG / "active.md"
        archive_idx = DOCS_BUG / "archive.md"
        archived_dir = DOCS_BUG / "archived"
        if active.is_file():
            all_entries.extend(parse_active_file(KIND_BUG, active))
        index = parse_archive_index(KIND_BUG, archive_idx) if archive_idx.is_file() else {}
        for entry in parse_archived_dir(KIND_BUG, archived_dir):
            all_entries.append(entry)

    if DOCS_FEATURE.is_dir():
        active = DOCS_FEATURE / "active.md"
        archive_idx = DOCS_FEATURE / "archive.md"
        archived_dir = DOCS_FEATURE / "archived"
        if active.is_file():
            all_entries.extend(parse_active_file(KIND_FEATURE, active))
        index = parse_archive_index(KIND_FEATURE, archive_idx) if archive_idx.is_file() else {}
        for entry in parse_archived_dir(KIND_FEATURE, archived_dir):
            all_entries.append(entry)

    # 按 (kind, id) 排序；同 (kind, id) 多源时全部保留（脚本会逐个迁移；幂等基于 source path）
    all_entries.sort(key=lambda e: (e.kind, e.id))
    return all_entries


# ---------------------------------------------------------------------------
# 草稿生成
# ---------------------------------------------------------------------------


def write_drafts(entries: list[Entry]) -> None:
    """写入 draft/<kind>_<id>.md 与 draft/summary.json。"""
    if DRAFT_DIR.exists():
        shutil.rmtree(DRAFT_DIR)
    DRAFT_DIR.mkdir(parents=True, exist_ok=True)

    summary: list[dict] = []
    seen_filenames: dict[str, int] = {}
    for e in entries:
        filename = e.draft_filename()
        # 避免同 (kind, id) 多次时覆盖；用后缀 _<n>
        if filename in seen_filenames:
            seen_filenames[filename] += 1
            filename = filename.replace(".md", f"__{seen_filenames[filename]}.md")
        else:
            seen_filenames[filename] = 0
        (DRAFT_DIR / filename).write_text(e.body, encoding="utf-8")
        summary.append({
            "kind": e.kind,
            "id": e.id,
            "title": e.title,
            "labels": e.labels(),
            "source": e.source,
            "draft_file": filename,
        })
    (DRAFT_DIR / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8"
    )


# ---------------------------------------------------------------------------
# 幂等：拉取远端已迁条目
# ---------------------------------------------------------------------------


def fetch_remote_migrated(repo: str) -> set[str]:
    """拉取所有 migrated-from:docs label 的 issue，提取其溯源 source path 集合。"""
    cmd = [
        "gh", "issue", "list",
        "--repo", repo,
        "--label", "migrated-from:docs",
        "--state", "all",
        "--json", "body",
        "--limit", "1000",
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, check=True)
    issues = json.loads(result.stdout)
    sources: set[str] = set()
    for issue in issues:
        body = issue.get("body", "") or ""
        m = SOURCE_MARKER.search(body)
        if m:
            sources.add(m.group("src"))
    return sources


def fetch_remote_migrated_map(repo: str) -> dict[str, int]:
    """拉取已迁 issue 的 source → issue_number 完整映射。

    与 fetch_remote_migrated 不同：返回 dict 而非 set，含 issue 编号。
    需用 gh api 拿到 number 字段（gh issue list 默认 json 不含 number + body 同请求）。
    """
    issues = []
    page = 1
    while True:
        # gh api 端点 labels= 必须用 query string（?labels=），-f 会被当 array
        endpoint = (
            f"repos/{repo}/issues?labels=migrated-from:docs"
            f"&state=all&per_page=100&page={page}"
        )
        cmd = [
            "gh", "api", endpoint,
            "--jq", "[.[] | {number, body}]",
        ]
        result = subprocess.run(cmd, capture_output=True, text=True, check=True)
        page_issues = json.loads(result.stdout)
        if not page_issues:
            break
        issues.extend(page_issues)
        if len(page_issues) < 100:
            break
        page += 1

    src_to_num: dict[str, int] = {}
    for issue in issues:
        body = issue.get("body", "") or ""
        m = SOURCE_MARKER.search(body)
        if m:
            src_to_num[m.group("src")] = issue["number"]
    return src_to_num


# ---------------------------------------------------------------------------
# Apply：调用 gh issue create
# ---------------------------------------------------------------------------


def gh_create_issue(repo: str, entry: Entry) -> int:
    """调用 gh issue create，返回 issue 编号。"""
    cmd = [
        "gh", "issue", "create",
        "--repo", repo,
        "--title", entry.title,
        "--body-file", "-",  # stdin
    ]
    for label in entry.labels():
        cmd.extend(["--label", label])
    proc = subprocess.run(
        cmd,
        input=entry.body,
        capture_output=True,
        text=True,
        check=True,
    )
    # gh 输出形如：https://github.com/owner/repo/issues/123
    m = re.search(r"/issues/(?P<n>\d+)\s*$", proc.stdout.strip())
    if not m:
        raise RuntimeError(f"无法解析 issue 编号：{proc.stdout!r}")
    return int(m.group("n"))


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="实际调用 gh issue create")
    parser.add_argument("--repo", default="rushsinging/aemeath", help="目标 GitHub 仓库")
    parser.add_argument("--limit", type=int, default=None, help="最多创建 issue 数（调试用）")
    parser.add_argument(
        "--sync-map",
        action="store_true",
        help="仅从远端拉 source→issue_number 映射并写回 migration-map.json，不创建 issue",
    )
    args = parser.parse_args()

    entries = collect_all_entries()
    print(f"[collect] 总计 {len(entries)} 条", file=sys.stderr)

    if not args.apply:
        # sync-map 是 --apply 的子集（要拉远端），所以走 apply 分支
        if not args.sync_map:
            # 纯 dry-run
            write_drafts(entries)
            # 写入初始 migration-map.json（issue_number 全 null）
            MAP_PATH.write_text(
                json.dumps([e.to_dict() for e in entries], ensure_ascii=False, indent=2),
                encoding="utf-8",
            )
            print(f"[dry-run] 草稿已写入 {DRAFT_DIR.relative_to(REPO_ROOT)}/", file=sys.stderr)
            print(f"[dry-run] 映射表已写入 {MAP_PATH.relative_to(REPO_ROOT)}", file=sys.stderr)
            return 0

    # apply
    if shutil.which("gh") is None:
        print("错误：未找到 gh CLI", file=sys.stderr)
        return 2

    # 幂等：拉远端已迁条目
    print(f"[apply] 拉取 {args.repo} 远端 migrated-from:docs issue ...", file=sys.stderr)
    remote = fetch_remote_migrated(args.repo)
    print(f"[apply] 远端已迁 {len(remote)} 条", file=sys.stderr)

    pending = [e for e in entries if e.source not in remote]
    skipped = [e for e in entries if e.source in remote]
    print(f"[apply] 待迁 {len(pending)} 条，跳过 {len(skipped)} 条（幂等）", file=sys.stderr)

    if args.sync_map:
        # 仅同步映射：拉远端已迁 source→issue_number 写回 entries
        src_to_num = fetch_remote_migrated_map(args.repo)
        synced = 0
        for e in entries:
            if e.source in src_to_num and e.issue_number is None:
                e.issue_number = src_to_num[e.source]
                synced += 1
        MAP_PATH.write_text(
            json.dumps([e.to_dict() for e in entries], ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        print(f"[sync-map] 同步 {synced} 条，映射表已更新", file=sys.stderr)
        return 0

    if args.limit:
        pending = pending[: args.limit]
        print(f"[apply] --limit {args.limit}，实际处理 {len(pending)} 条", file=sys.stderr)

    # 同步写草稿，方便后续 PR review
    write_drafts(pending)

    # 创建 issue（失败不中断，便于断点续传）
    success_count = 0
    fail_count = 0
    failed: list[Entry] = []
    for e in pending:
        try:
            n = gh_create_issue(args.repo, e)
            e.issue_number = n
            success_count += 1
            print(f"  [ok] {e.kind} #{e.id} → issue #{n}", file=sys.stderr)
        except subprocess.CalledProcessError as ex:
            print(f"  [fail] {e.kind} #{e.id}: {ex.stderr.strip()}", file=sys.stderr)
            failed.append(e)
            fail_count += 1
        except Exception as ex:  # noqa: BLE001
            print(f"  [fail] {e.kind} #{e.id}: {ex}", file=sys.stderr)
            failed.append(e)
            fail_count += 1

    # 写最终映射（即使有 fail 也写，便于 retry 续传）
    MAP_PATH.write_text(
        json.dumps([e.to_dict() for e in entries], ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    print(
        f"[apply] 成功 {success_count}，失败 {fail_count}；"
        f"映射表已更新：{MAP_PATH.relative_to(REPO_ROOT)}",
        file=sys.stderr,
    )
    if failed:
        print(f"[apply] 失败条目 source 列表：", file=sys.stderr)
        for e in failed:
            print(f"  - {e.source}", file=sys.stderr)
        return 1
    return 0

if __name__ == "__main__":
    sys.exit(main())
