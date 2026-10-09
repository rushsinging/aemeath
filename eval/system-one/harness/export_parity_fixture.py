#!/usr/bin/env python3
"""导出 79-case fixture（#1833 批次 2 Rust 回归门禁的数值真相源）。

产出 `eval/system-one/fixtures/parity_q8/`：
- `manifest.json`：维度 / temperature / case 数与生成口径（含 backbone revision）
- `79_cases.jsonl`：每 case 一行——row token ids（kev encode 口径）、decide/opts
  位置、golden hidden（torch fp32 前向）、golden probs（同 hidden 经 Python
  PointerHead softmax，与 Rust 对拍同口径自洽）
- `head/head.f32`：PointerHead 权重裸二进制（q_weight | q_bias | k_weight |
  k_bias，f32 LE，row-major）

零网络：hidden 来自合并后 backbone 的 torch fp32 前向（LoRA 合并正确性已在
批次 1 对分锁定），不依赖 kev.serve / llama-server。

用法：
    python3 export_parity_fixture.py --kev-repo ~/.cache/system-one-eval/kev \
        --backbone ~/.cache/system-one-eval/kev-merged-fp32-backbone \
        --datasets ../datasets/memory_rerank.jsonl [--datasets ...] \
        [--out ../fixtures/parity_q8]
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from pathlib import Path

import torch

HEAD_PT = Path.home() / ".cache/huggingface/hub/models--jaredpalmer--kev-0.8b/snapshots" \
    / "bf75a6a8848ea6960ff2ed108d9ed44c2941174f/head.pt"
BASE = "Qwen/Qwen3.5-0.8B-Base"
BASE_REVISION = "dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68"


class PointerHead:
    """kev/model.py:205 的独立复刻（与 parity_gguf.py 同一实现）。"""

    def __init__(self, head_pt: Path):
        blob = torch.load(head_pt, map_location="cpu", weights_only=False)
        head = blob["head"]
        self.q_weight = head["q.weight"].float()  # [256, 1024]
        self.q_bias = head["q.bias"].float()
        self.k_weight = head["k.weight"].float()
        self.k_bias = head["k.bias"].float()
        self.temperature = float(blob["temperature"])

    def logits(self, h_decide: torch.Tensor, h_opts: torch.Tensor) -> torch.Tensor:
        q = self.q_weight @ h_decide + self.q_bias
        k = h_opts @ self.k_weight.T + self.k_bias
        return (k @ q) / (self.q_weight.shape[0] ** 0.5) / self.temperature


def case_payload(case: dict) -> dict:
    """dataset jsonl case -> jev payload（与 parity_gguf.py 完全同构）。"""
    scenario = case.get("scenario", "")
    if "stop_verify" in scenario:
        return {"state": case["state"],
                "questions": {"q": {"type": "noul", "instructions":
                                    ("Based on the task and the agent's actions described in the "
                                     "state, is the task fully complete? Answer true only if all "
                                     "requirements are met.")}}}
    if "permission" in scenario:
        return {"state": case["state"],
                "questions": {"q": {"type": "noul", "instructions":
                                    ("Does the requested operation have destructive or irreversible "
                                     "side effects?")}}}
    if "memory_rerank" in scenario:
        instr = ("Given a user message from a coding-agent session, "
                 "retrieve the most relevant memory.")
    else:
        instr = "Which option is the most relevant answer to the question?"
    options = {str(i): text for i, text in enumerate(case["answers"])}
    return {"state": case["context"] + "\n" + case["question"],
            "questions": {"q": {"type": "choice", "instructions": instr, "criteria": options}}}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--kev-repo", required=True)
    ap.add_argument("--backbone", required=True, help="合并后 backbone-only 目录（torch fp32）")
    ap.add_argument("--datasets", nargs="+", required=True)
    ap.add_argument("--out", default=str(Path(__file__).parent.parent / "fixtures/parity_q8"))
    args = ap.parse_args()

    sys.path.insert(0, args.kev_repo)
    import kev.api as kev_api  # noqa: E402
    import kev.model as kev_model  # noqa: E402
    from transformers import AutoModel  # noqa: E402

    tok = kev_model.load_tokenizer(BASE, revision=BASE_REVISION)
    head = PointerHead(HEAD_PT)
    lm = AutoModel.from_pretrained(args.backbone, dtype=torch.float32,
                                   attn_implementation="eager")
    lm.eval()
    print(f"PointerHead temperature={head.temperature:.7f}")

    out_dir = Path(args.out)
    (out_dir / "head").mkdir(parents=True, exist_ok=True)

    # head.f32：q_weight | q_bias | k_weight | k_bias（f32 LE row-major）
    tensors = [head.q_weight, head.q_bias, head.k_weight, head.k_bias]
    blob = b"".join(t.detach().cpu().numpy().astype("<f4").tobytes() for t in tensors)
    (out_dir / "head/head.f32").write_bytes(blob)

    rows_out = []
    for ds in args.datasets:
        cases = [json.loads(line) for line in open(ds) if line.strip()]
        name = Path(ds).stem
        for case in cases:
            payload = case_payload(case)
            req = kev_api.SystemOneRequest(**payload)
            rec, _meta = kev_api.to_record(req)
            enc = kev_model.encode(tok, rec, max_state=kev_model.SERVE_MAX_STATE,
                                   max_branch=kev_model.SERVE_MAX_BRANCH)
            state_ids, _state_pos, rows = kev_model.rows_of(enc)
            row = rows[0]  # 与 parity_gguf.py 同口径：单题单 row
            row_ids = state_ids + row["ids"]
            with torch.no_grad():
                hidden = lm(input_ids=torch.tensor([row_ids])).last_hidden_state[0].float()
            base = len(state_ids)
            h_decide = hidden[base + row["decide"]]
            h_opts = torch.stack([hidden[base + o] for o in row["opts"]])
            probs = torch.softmax(head.logits(h_decide, h_opts), dim=-1)
            gold = case.get("gold")
            if gold is None:
                gold = 1 if case.get("label") else 0
            rows_out.append({
                "dataset": name,
                "id": case.get("id"),
                "gold": gold,
                "n_options": len(row["opts"]),
                "ids": row_ids,
                "decide": base + row["decide"],
                "opts": [base + o for o in row["opts"]],
                "golden_probs": [round(p, 7) for p in probs.tolist()],
                "decide_hidden": [round(v, 7) for v in h_decide.tolist()],
                "options_hidden": [round(v, 7) for v in h_opts.reshape(-1).tolist()],
            })
            print(f"[{name}] {case.get('id')}: n_opts={len(row['opts'])} "
                  f"row_len={len(row_ids)} argmax={int(probs.argmax())}")

    with open(out_dir / "79_cases.jsonl", "w") as f:
        for row in rows_out:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")

    manifest = {
        "schema_version": 1,
        "case_count": len(rows_out),
        "hidden_size": int(head.q_weight.shape[1]),
        "pointer_dimension": int(head.q_weight.shape[0]),
        "temperature": head.temperature,
        "source": {
            "backbone": "Qwen/Qwen3.5-0.8B-Base@" + BASE_REVISION,
            "lora": "jaredpalmer/kev-0.8b (merged fp32)",
            "hidden_source": "torch fp32 eager forward",
            "probs_source": "python PointerHead softmax (same hidden)",
            "gguf_target": "Q8_0",
            "datasets": [Path(ds).name for ds in args.datasets],
            "generator": "harness/export_parity_fixture.py",
        },
        "head_layout": {
            "file": "head/head.f32",
            "order": ["q_weight", "q_bias", "k_weight", "k_bias"],
            "dtype": "<f4",
            "q_weight_shape": [256, 1024],
        },
    }
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    print(f"\n写入 {out_dir}: {len(rows_out)} cases")


if __name__ == "__main__":
    main()
