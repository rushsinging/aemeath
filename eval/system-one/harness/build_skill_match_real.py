#!/usr/bin/env python3
"""skill_match_real 真实目录复验数据集构造（#1835）。

候选 = ~/.agents/skills 全量真实 skill（name: description，对齐生产 criteria 口径）；
query = 真实 ToolSearch 使用意图（中英文、间接语义、近义词——词法规则难以命中的形态）；
gold = 人工标注的目标 skill。词法基线（compute_relevance 硬规则）对照同期输出。

用法：python3 harness/build_skill_match_real.py
"""
from __future__ import annotations

import json
import pathlib
import re

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent
SKILLS_DIR = pathlib.Path.home() / ".agents" / "skills"

# (真实意图 query, gold skill name)
QUERIES: list[tuple[str, str]] = [
    ("帮我把这个 PR 合了然后清理掉分支", "merge"),
    ("打开仓库里的架构设计文档", "open"),
    ("重新编译并安装 aemeath CLI 二进制", "build-cli"),
    ("把网页操作自动化，登录后点按钮填表单", "agent-browser"),
    ("面试打磨我的计划，并把决策沉淀成 ADR 文档", "grill-with-docs"),
    ("找一个能画时序图的工具", "archify"),
    ("退出 promptfolio 的登录", "promptfolio-logout"),
    ("e2e test stuck at login selector, debug it", "playwright"),
    ("把 gitignore 里加上 CLAUDE.md", "fix-gitignore"),
    ("看看谁擅长 rust 后端，找个人协作", "promptfolio-search-people"),
    ("publish the release notes to feishu", "release-pub"),
    ("worktree 磁盘满了，清理孤儿构建缓存", "clean-worktree"),
    ("复制一个现有游戏到开发环境", "wanaka-replicate"),
]


PROJECT_SKILLS_DIR = pathlib.Path(
    "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.agents/skills")


def load_skills() -> dict[str, str]:
    """name -> description（全局 + aemeath 项目两级 SKILL.md；兼容单行与 >- 折叠）。"""
    skills: dict[str, str] = {}
    for base in (SKILLS_DIR, PROJECT_SKILLS_DIR):
        for entry in sorted(base.iterdir()):
            skill_md = entry / "SKILL.md"
            if not skill_md.exists() or entry.name in skills:
                continue
            lines = skill_md.read_text(encoding="utf-8").splitlines()
            description_parts: list[str] = []
            collecting = False
            for line in lines:
                if line.startswith("description:"):
                    value = line[len("description:"):].strip()
                    if value in (">-", ">", "|", "|-"):
                        collecting = True
                    elif value:
                        description_parts.append(value)
                    break_at_next_key = True
                elif collecting and line.startswith("  "):
                    description_parts.append(line.strip())
                elif collecting:
                    break
            if description_parts:
                skills[entry.name] = " ".join(description_parts)
    return skills


def main() -> None:
    skills = load_skills()
    missing = [gold for _, gold in QUERIES if gold not in skills]
    assert not missing, f"gold skill 缺失: {missing}"

    names = sorted(skills)
    cases = []
    for index, (query, gold) in enumerate(QUERIES, start=101):
        answers = [f"{name}: {skills[name]}" for name in names]
        cases.append({
            "id": f"skill-r{index}",
            "context": query,
            "question": "哪个 skill 最匹配该意图？",
            "answers": answers,
            "gold": names.index(gold),
            "scenario": "skill_match_real",
        })

    path = EVAL_ROOT / "datasets" / "skill_match_real.jsonl"
    with path.open("w", encoding="utf-8") as fh:
        for case in cases:
            fh.write(json.dumps(case, ensure_ascii=False) + "\n")
    print(f"cases: {len(cases)} | skills pool: {len(names)} -> {path}")

    # 词法基线对照（ToolSearch 硬规则：exact=100/name contains=80/desc contains=50）
    hits = 0
    for query, gold in QUERIES:
        lower = query.lower()
        scored = []
        for name in names:
            if name.lower() == lower:
                scored.append((100.0, name))
            elif name.lower() in lower:
                scored.append((80.0, name))
            elif lower in skills[name].lower():
                scored.append((50.0, name))
        top = max(scored, default=None)
        hit = top is not None and top[1] == gold and top[0] >= 50
        hits += int(hit)
        print(f"  {query[:28]:<30} gold={gold:<24} lexical={'✓' if hit else '✗'} "
              f"(top={top[1] if top else '无命中'})")
    print(f"词法基线 R@1 = {hits}/{len(QUERIES)} = {hits / len(QUERIES):.3f}")


if __name__ == "__main__":
    main()
