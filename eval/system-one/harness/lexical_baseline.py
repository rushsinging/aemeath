#!/usr/bin/env python3
"""词法召回基线（#1834 真实会话复验）：Python 复刻 memory BM25 词法搜索。

复刻对象：agent/features/memory/src/domain/lexical_search.rs
- tokenize：lowercase；CJK 连续段 → bigram（单字直入）；其余字母数字连续 → 词
- 文档向量：content（权重 3.0）+ tags（2.0）+ facet（category/layer Debug 名，1.0）
- BM25：k1=1.2、b=0.75；idf=ln((N-df+0.5)/(df+0.5)+1)
- exact match（content 全文 trim+lower == query）+100
- 仅保留 score>0；降序，tie → search_tie_break_score（pinned/confirmation/recency），再 id 升序

用途：对真实 query 产出词法 top-N 候选序，与 kev 重排结果对比，
回答「rerank 相比现状词法序是否改善」。近似基线：未经 Rust 逐分对拍。
"""
from __future__ import annotations

import json
import math

BM25_K1 = 1.2
BM25_B = 0.75
CONTENT_WEIGHT = 3.0
TAG_WEIGHT = 2.0
FACET_WEIGHT = 1.0
EXACT_MATCH_BOOST = 100.0

CATEGORY_DEBUG = {
    "fact": "Fact", "decision": "Decision", "preference": "Preference",
    "pattern": "Pattern", "pitfall": "Pitfall",
}
LAYER_DEBUG = {"global": "Global", "project": "Project"}


def is_han(ch: str) -> bool:
    cp = ord(ch)
    return (0x3400 <= cp <= 0x4DBF or 0x4E00 <= cp <= 0x9FFF
            or 0xF900 <= cp <= 0xFAFF or 0x20000 <= cp <= 0x2A6DF
            or 0x2A700 <= cp <= 0x2B73F or 0x2B740 <= cp <= 0x2B81F
            or 0x2B820 <= cp <= 0x2CEAF or 0x2CEB0 <= cp <= 0x2EBEF
            or 0x30000 <= cp <= 0x3134F)


def tokenize(text: str) -> list[str]:
    terms: list[str] = []
    word: list[str] = []
    han_run: list[str] = []

    def flush_word():
        if word:
            terms.append("".join(word))
            word.clear()

    def flush_han():
        if len(han_run) == 1:
            terms.append(han_run[0])
        elif len(han_run) > 1:
            terms.extend("".join(pair) for pair in zip(han_run, han_run[1:]))
        han_run.clear()

    for ch in text.lower():
        if is_han(ch):
            flush_word()
            han_run.append(ch)
        else:
            flush_han()
            if ch.isalnum():
                word.append(ch)
            else:
                flush_word()
    flush_word()
    flush_han()
    return terms


def recency_score(last_confirmed_at: int, now: int) -> int:
    days = max(0, now - last_confirmed_at) // 86400
    if days == 0:
        return 1000
    if days <= 7:
        return 800
    if days <= 30:
        return 500
    if days <= 90:
        return 200
    return 50


def tie_break_score(entry: dict, now: int) -> int:
    pinned_bonus = 10_000 if entry.get("pinned") else 0
    confirmation = min(entry.get("confirmation_count", 0), 20) * 100
    return pinned_bonus + confirmation + recency_score(entry.get("last_confirmed_at", 0), now)


def rank(entries: list[dict], query: str, now: int, limit: int = 10) -> list[int]:
    """返回按词法分降序的 entry 索引列表（score>0）。"""
    query_terms = tokenize(query)
    if not query_terms or limit == 0 or not entries:
        return []

    docs = []
    for entry in entries:
        facet = f"{CATEGORY_DEBUG.get(entry.get('category', ''), '')} " \
                f"{LAYER_DEBUG.get(entry.get('layer', ''), '')}"
        weighted: dict[str, float] = {}
        for term in tokenize(entry["content"]):
            weighted[term] = weighted.get(term, 0.0) + CONTENT_WEIGHT
        for tag in entry.get("tags", []):
            for term in tokenize(tag):
                weighted[term] = weighted.get(term, 0.0) + TAG_WEIGHT
        for term in tokenize(facet):
            weighted[term] = weighted.get(term, 0.0) + FACET_WEIGHT
        length = (len(tokenize(entry["content"])) + len(tokenize(facet))
                  + sum(len(tokenize(t)) for t in entry.get("tags", [])))
        docs.append({"entry": entry, "weighted": weighted,
                     "unique": set(weighted), "length": length})

    total = len(docs)
    average_length = sum(d["length"] for d in docs) / total
    normalized_query = query.strip().lower()

    scored = []
    for index, doc in enumerate(docs):
        score = 0.0
        for term in query_terms:
            tf = doc["weighted"].get(term, 0.0)
            if tf == 0.0:
                continue
            df = sum(1 for d in docs if term in d["unique"])
            idf = math.log((total - df + 0.5) / (df + 0.5) + 1.0)
            length_norm = 1.0 - BM25_B + BM25_B * doc["length"] / average_length
            score += idf * tf * (BM25_K1 + 1.0) / (tf + BM25_K1 * length_norm)
        if doc["entry"]["content"].strip().lower() == normalized_query:
            score += EXACT_MATCH_BOOST
        if score > 0.0:
            scored.append((index, score))

    scored.sort(key=lambda pair: (
        -pair[1],
        -tie_break_score(documents_entry(docs, pair[0]), now),
        documents_entry(docs, pair[0]).get("id", ""),
    ))
    return [index for index, _ in scored[:limit]]


def documents_entry(docs: list[dict], index: int) -> dict:
    return docs[index]["entry"]


def main() -> None:
    import os
    import sys
    import time

    bucket = os.path.expanduser(
        "~/.agents/memory/v2_2a2bdf23f2c25a16214aa03ae0f51936d9875c766dea4e2fc0bd779ea1b481ce")
    manifest = json.load(open(f"{bucket}/primary/manifest.json"))
    active_digest = next(e["内容摘要"] for e in manifest["成员证据"] if e["名称"] == "active")
    entries = json.load(open(f"{bucket}/members/{active_digest}"))["entries"]

    dataset_path = sys.argv[1] if len(sys.argv) > 1 else "datasets/memory_rerank.jsonl"
    cases = [json.loads(line) for line in open(dataset_path)
             if json.loads(line)["id"].startswith("mem-r")]
    now = int(time.time())

    hits_at_1 = 0
    for case in cases:
        ranking = rank(entries, case["context"], now, limit=5)
        gold_content = case["answers"][case["gold"]]
        gold_index = next(i for i, e in enumerate(entries)
                          if e["content"][:500] == gold_content)
        position = ranking.index(gold_index) + 1 if gold_index in ranking else None
        hits_at_1 += 1 if position == 1 else 0
        print(f"{case['id']}: 词法 top5={ranking} gold=entry[{gold_index}] "
              f"位置={position or '未入top5'}")
    print(f"\n词法基线 R@1 = {hits_at_1}/{len(cases)}")


if __name__ == "__main__":
    main()
