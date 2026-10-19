#!/usr/bin/env python3
"""冻结 Memory 清单指向的 active 现场，供双通道影子评测重放。

不按 members 目录 mtime 选版本；只读取 primary manifest 引用的成员，
校验长度与 SHA-256，并记录全局和项目 corpus 的修订。仅读源文件。
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import time

SNAPSHOT_SCHEMA_VERSION = 1


def is_injection_eligible(entry: dict, now: int) -> bool:
    ttl = entry.get("ttl")
    ttl_seconds = ttl.get("secs", 0) if isinstance(ttl, dict) else ttl
    expired = ttl_seconds is not None and now > entry["created_at"] + ttl_seconds
    return not entry.get("outdated", False) and not entry.get("superseded_by") and not expired


def load_snapshot(path: Path) -> dict:
    snapshot = json.loads(path.read_text(encoding="utf-8"))
    if snapshot.get("schema_version") != SNAPSHOT_SCHEMA_VERSION:
        raise ValueError("unsupported snapshot schema")
    if not isinstance(snapshot.get("captured_at"), int):
        raise ValueError("snapshot requires captured_at in seconds")
    if not isinstance(snapshot.get("entries"), list):
        raise ValueError("snapshot requires active entries")
    return snapshot


def capture_corpus(corpus: Path) -> tuple[dict, list[dict]]:
    manifest_path = corpus / "primary" / "manifest.json"
    manifest_bytes = manifest_path.read_bytes()
    manifest = json.loads(manifest_bytes)
    evidence = next(member for member in manifest["成员证据"] if member["名称"] == "active")
    digest = evidence["内容摘要"]
    if len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
        raise ValueError("invalid active content digest")
    member_bytes = (corpus / "members" / digest).read_bytes()
    if len(member_bytes) != evidence["字节数"]:
        raise ValueError("active member length mismatch")
    content_digest = hashlib.sha256(
        b"aemeath.storage.dataset.member.digest.v1\0"
        + len(member_bytes).to_bytes(8, "little") + member_bytes
    ).hexdigest()
    if content_digest != digest:
        raise ValueError("active member digest mismatch")
    member = json.loads(member_bytes)
    if member.get("location") != "active":
        raise ValueError("manifest active member has wrong location")
    if manifest_path.read_bytes() != manifest_bytes:
        raise ValueError("corpus changed during capture; retry capture")
    return {
        "corpus": corpus.name,
        "revision": manifest["修订号"],
        "active_sha256": digest,
        "entries": len(member["entries"]),
    }, member["entries"]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", action="append", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    snapshot = {"schema_version": SNAPSHOT_SCHEMA_VERSION, "captured_at": int(time.time()),
                "corpora": [], "entries": []}
    for corpus in args.corpus:
        metadata, entries = capture_corpus(corpus)
        snapshot["corpora"].append(metadata)
        snapshot["entries"].extend(entries)
    identities = [entry["id"] for entry in snapshot["entries"]]
    if len(identities) != len(set(identities)):
        raise ValueError("duplicate active entry identity across corpora")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(snapshot, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"captured {len(identities)} active entries from {len(args.corpus)} corpora -> {args.output}")


if __name__ == "__main__":
    main()
