#!/usr/bin/env python3
"""从 session dataset 读取压缩输入快照（#2133 P1）。

dataset 是 content-addressed 存储：
  <session>.dataset/primary/manifest.json      成员名 → 内容摘要
  <session>.dataset/members/<内容摘要>         实际 JSON blob

本模块只做「读」：把 session 的 committed steps 还原成压缩前消息序列，
并带上当前 compact summary（下一次压缩的 previous_summary）。
不评价压缩结果，也不修改任何文件。
"""
from __future__ import annotations

import json
import os
from pathlib import Path

CJK_RANGES = (
    (0x3000, 0x303F),  # 中文标点
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xF900, 0xFAFF),
    (0xFF00, 0xFFEF),  # 全角
)


def estimate_tokens(text: str) -> int:
    """与 runtime 估算口径对齐：CJK 字符 ×0.6，其余 ×0.3（向上取整）。"""
    cjk = 0
    other = 0
    for char in text:
        code = ord(char)
        if any(start <= code <= end for start, end in CJK_RANGES):
            cjk += 1
        else:
            other += 1
    return int(cjk * 0.6 + other * 0.3 + 0.999)


def _load_blob(dataset: Path, digest: str) -> dict:
    return json.loads((dataset / "members" / digest).read_text(encoding="utf-8"))


def _manifest_index(dataset: Path) -> dict[str, str]:
    manifest = json.loads(
        (dataset / "primary" / "manifest.json").read_text(encoding="utf-8")
    )
    return {entry["名称"]: entry["内容摘要"] for entry in manifest["成员证据"]}


def load_session(dataset: Path) -> dict:
    """读取 session：返回 id、compact summary、按提交顺序的 steps。"""
    index = _manifest_index(dataset)
    state = _load_blob(dataset, index["session-state.json"])
    metadata = _load_blob(dataset, index.get("metadata.json", index["session-state.json"]))
    compact = state.get("compact") or {}
    committed = state.get("committed_steps") or []

    steps = []
    for ref in committed:
        name = step_member_name(ref["run_id"], ref["step_id"])
        digest = index.get(name)
        if digest is None:
            continue
        step = _load_blob(dataset, digest).get("step") or {}
        outcome = step.get("outcome") or {}
        messages = outcome.get("messages") or []
        if not messages:
            continue
        steps.append(
            {
                "run_id": ref["run_id"],
                "step_id": ref["step_id"],
                "finalize_cause": outcome.get("finalize_cause"),
                "messages": messages,
            }
        )

    return {
        "session_id": metadata.get("id") or state.get("id") or dataset.stem,
        "model": ((metadata.get("metadata") or {}).get("model")),
        "previous_summary": compact.get("summary") if isinstance(compact, dict) else None,
        "committed_step_count": len(committed),
        "steps": steps,
    }


def step_member_name(run_id: str, step_id: str) -> str:
    """与 runtime 的 step 成员命名保持一致（hex 编码）。"""
    return f"step-{run_id.encode().hex()}-{step_id.encode().hex()}.json"


def message_text(message: dict) -> str:
    """把一条消息的 content blocks 摊平成文本，用于抽取 must_keep 与估算。"""
    parts = []
    for block in message.get("content") or []:
        block_type = block.get("type")
        if block_type in ("text", "thinking"):
            parts.append(block.get("text") or block.get("thinking") or "")
        elif block_type == "tool_use":
            parts.append(json.dumps(block.get("input") or {}, ensure_ascii=False))
            parts.append(block.get("name") or "")
        elif block_type == "tool_result":
            parts.append(block.get("text") or "")
    return "\n".join(part for part in parts if part)


def flatten_messages(session: dict) -> list[dict]:
    messages = []
    for step in session["steps"]:
        messages.extend(step["messages"])
    return messages


def iter_datasets(root: Path):
    for path in sorted(root.glob("*/*.dataset")):
        if (path / "primary" / "manifest.json").is_file():
            yield path


def dataset_size_hint(dataset: Path) -> int:
    members = dataset / "members"
    if not members.is_dir():
        return 0
    return sum(entry.stat().st_size for entry in os.scandir(members) if entry.is_file())
