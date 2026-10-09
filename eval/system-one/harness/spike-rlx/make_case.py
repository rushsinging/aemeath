#!/usr/bin/env python3
"""Build the mem-001 spike case for spike-rlx (Rust side).

Reuses harness/parity_gguf.py encode + PointerHead logic (kev.model.encode,
case_payload, GgufHidden) so the token ids / indices match the 79-case
llama.cpp↔MLX parity exactly.

Outputs spike-rlx/data/case_mem001.json:
    {id, row_ids, decide, opts, golden_probs, llama_probs}

    golden_probs : kev serve :8009 (MLX golden, read-only query)
    llama_probs  : llama-server :8019 hidden -> PointerHead(head.pt) -> softmax
                   (= parity_gguf.py gguf 口径 reference for the Rust run)

Usage: convert-venv/bin/python make_case.py [--case mem-001]
"""
import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
HARNESS = HERE.parent                      # eval/system-one/harness
DATASETS = HARNESS.parent / "datasets"     # eval/system-one/datasets
KEV_REPO = Path.home() / ".cache/system-one-eval/kev"

sys.path.insert(0, str(HARNESS))
sys.path.insert(0, str(KEV_REPO))

import parity_gguf as P  # noqa: E402


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--case", default="mem-001")
    ap.add_argument("--dataset", default=str(DATASETS / "memory_rerank.jsonl"))
    ap.add_argument("--out", default=str(HERE / "data" / "case_mem001.json"))
    args = ap.parse_args()

    import torch
    import kev.api as kev_api  # noqa: E402
    import kev.model as kev_model  # noqa: E402

    cases = [json.loads(line) for line in open(args.dataset) if line.strip()]
    case = next(c for c in cases if c.get("id") == args.case)
    payload = P.case_payload(case)

    # 1) golden probs from kev serve (:8009, read-only query)
    golden = P.payload_probs_golden(payload)

    # 2) encode exactly like parity_gguf.py / serve
    tok = kev_model.load_tokenizer(P.BASE, revision=P.BASE_REVISION)
    req = kev_api.SystemOneRequest(**payload)
    rec, _meta = kev_api.to_record(req)
    enc = kev_model.encode(tok, rec, max_state=kev_model.SERVE_MAX_STATE,
                           max_branch=kev_model.SERVE_MAX_BRANCH)
    state_ids, _state_pos, rows = kev_model.rows_of(enc)
    print(f"state={len(state_ids)} tokens, rows={len(rows)}, "
          f"row lens={[len(r['ids']) for r in rows]}")
    row = rows[0]  # parity_gguf.py returns all_probs[0]
    base = len(state_ids)
    row_ids = [int(t) for t in state_ids + row["ids"]]
    decide = base + int(row["decide"])
    opts = [base + int(o) for o in row["opts"]]
    print(f"row_ids={len(row_ids)} decide@{decide} opts@{opts}")

    # 3) llama-server hidden -> PointerHead -> probs (gguf 口径 reference)
    head = P.PointerHead(P.HEAD_PT)
    h = P.GgufHidden().hidden(row_ids)
    h_decide = h[decide]
    h_opts = torch.stack([h[o] for o in opts])
    probs_llama = torch.softmax(head.logits(h_decide, h_opts), dim=-1).tolist()

    out = {
        "id": args.case,
        "row_ids": row_ids,
        "decide": decide,
        "opts": opts,
        "golden_probs": [float(x) for x in golden],
        "llama_probs": [float(x) for x in probs_llama],
    }
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(out, ensure_ascii=False) + "\n")
    print(f"golden : {[round(float(x),5) for x in golden]}")
    print(f"llama  : {[round(float(x),5) for x in probs_llama]}")
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
