#!/usr/bin/env python3
"""把**手工整理的判别 case**（`curated_cases.jsonl`）并进 `fixtures/legado_ab/`。

`make_corpus.py` 是从整批对拍产物里**取样**，覆盖不到「语料里本来就没有」的语义
（D10/D11/D15/D16/D21/D22 这些只有文档没有 case）。这些 case 手写在仓库里，
oracle 用同一个 JVM harness 录（`ICU_SHIM=1`）。

流程（harness 就绪后一条条跑）：

    ICU_SHIM=1 scripts/ab/harness/run.sh < scripts/ab/curated_cases.jsonl \\
        > target/legado-ab/curated_legado.jsonl
    python3 scripts/ab/merge_curated.py target/legado-ab/curated_legado.jsonl
    # 再把 examples/legado_ab.rs 的 EXPECTED_CASES 改成语料实际条数，然后重新基线：
    cargo run -q --example legado_ab -- --corpus fixtures/legado_ab --emit fixtures/legado_ab/rust.jsonl

幂等：同一个 id 重复跑就替换，不会越并越多。

**没有进 curated 的 D 项，以及为什么**：

- `D10`：只覆盖了**内存上限**那一半（`curated:d10:memory`，80MB 字符串——旧上限 64MiB 会 OOM、
  新上限 256MiB 能过）；「Context 缓存只留 3 个 + 淘汰时 run_gc」是**长跑**性质，单条 case 判别不了，
  已有 `examples/js_eval.rs` 的内存断言守着（报 `out of memory` 而不炸进程）。
- `D15`：只覆盖了**垫片 API 面**（`curated:d15:connect-chain`：链式 `timeout` / `header` 不炸、
  不真发请求）；真发 HTTP 的那半不可能进语料——对拍语料**故意不联网**
  （两侧都不接网络，保持离线可重复），`js_eval` 用假 `HttpFetcher` 覆盖「请求真的发出去了」。
"""

from __future__ import annotations

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
CURATED = os.path.join(HERE, "curated_cases.jsonl")
CORPUS = os.path.join(REPO, "fixtures", "legado_ab")


def load(path):
    records = {}
    if not os.path.exists(path):
        return records
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if line:
                obj = json.loads(line)
                records[obj["id"]] = obj
    return records


def write(path, records, order):
    with open(path, "w", encoding="utf-8") as handle:
        for cid in order:
            handle.write(json.dumps(records[cid], ensure_ascii=False) + "\n")


def main():
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[0], file=sys.stderr)
        print("用法：merge_curated.py <harness 录的 curated oracle.jsonl>", file=sys.stderr)
        return 1
    recorded = load(sys.argv[1])
    if not recorded:
        print(f"{sys.argv[1]} 里没有 oracle 记录", file=sys.stderr)
        return 1

    curated = load(CURATED)
    cases_path = os.path.join(CORPUS, "cases.jsonl")
    legado_path = os.path.join(CORPUS, "legado.jsonl")
    cases = load(cases_path)
    legado = load(legado_path)

    cases_order = list(cases)
    legado_order = list(legado)
    added = 0
    for cid, case in curated.items():
        oracle = recorded.get(cid)
        if oracle is None:
            print(f"curated case {cid} 没有 oracle 记录，先跑 harness", file=sys.stderr)
            return 1
        if cid not in cases:
            cases_order.append(cid)
            legado_order.append(cid)
            added += 1
        cases[cid] = case
        legado[cid] = oracle

    write(cases_path, cases, cases_order)
    write(legado_path, legado, legado_order)
    print(f"语料 {len(cases_order)} 条（新并 {added} 条 curated）；记得改 EXPECTED_CASES 并重新基线")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
