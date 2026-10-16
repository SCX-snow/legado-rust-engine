#!/usr/bin/env python3
"""从全量真机重测的产物里切一份**可提交的真实页面语料**（`fixtures/legado_ab_real/`）。

    scripts/ab/full_ab.sh 700                     # 先跑一遍全量重测（要网络，产物在 target/）
    python3 scripts/ab/make_real_corpus.py        # 再切语料

输入（`target/legado-ab-full/`）：`cases.jsonl` / `legado_out.jsonl` / `rust_out.jsonl`；
输出：`cases.jsonl` + `legado.jsonl`（真机 oracle 由 `ICU_SHIM=1 scripts/ab/harness/run.sh` 录）。

跟 `fixtures/legado_ab/` 的分工：那份是**合成**的、刻意偏向分歧的规则语料；这份是**真实书源 +
真实页面**的小切片，用来给真实页面形态（编码 / CDATA / 统计像素 / JSON API / 长文本）留回归。
所以挑选规则是：

1. **先挑我们与 legado 不一致的**（`DIFF_OK` / `BOTH_ERR` / 单边报错），它们才有回归价值；
2. 再按**源**轮转补齐，尽量多源、每个源最多 `--per-source` 条（同一个页面在多个 case 里会重复占体积）；
3. 页面（`content`）超过 `--max-content` 字节的不要（提交语料要控制体积）；
4. 规则里带 `org.jsoup.Jsoup.connect(` 的**不要**：legado 的 harness 那边这会真联网，语料就不确定了
   （`AnalyzeUrl` / `java.ajax` 是显式抛错桩，两边都不联网，可以留）；
5. 带时间/随机（`Date.now` / `currentTimeMillis` / `new Date(`）的也不要：进提交语料后每次跑值都变。
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import sys

STATUS_ORDER = ["DIFF_OK", "BOTH_ERR", "A_OK_B_ERR", "A_ERR_B_OK", "SAME"]


def load(path: str) -> dict[str, dict]:
    with open(path, encoding="utf-8") as handle:
        return {obj["id"]: obj for obj in (json.loads(line) for line in handle if line.strip())}


def canonical(outcome: dict) -> str | None:
    if not outcome.get("ok"):
        return None
    value = outcome.get("result")
    if isinstance(value, list):
        return "\x1f".join(str(item) for item in value)
    return "" if value is None else str(value)


def status(legado: dict, rust: dict) -> str:
    a, b = legado.get("ok"), rust.get("ok")
    if a and b:
        return "SAME" if canonical(legado) == canonical(rust) else "DIFF_OK"
    if a:
        return "A_OK_B_ERR"
    if b:
        return "A_ERR_B_OK"
    return "BOTH_ERR"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--in", dest="source_dir", default="target/legado-ab-full")
    parser.add_argument("--out", default="fixtures/legado_ab_real")
    parser.add_argument("--cases", type=int, default=140, help="最多收录多少条")
    parser.add_argument("--per-source", type=int, default=1)
    parser.add_argument("--max-content", type=int, default=40_000)
    parser.add_argument("--max-total", type=int, default=1_000_000, help="content 总字节上限")
    args = parser.parse_args()

    cases = load(os.path.join(args.source_dir, "cases.jsonl"))
    legado = load(os.path.join(args.source_dir, "legado_out.jsonl"))
    rust = load(os.path.join(args.source_dir, "rust_out.jsonl"))

    buckets: dict[str, dict[str, list[str]]] = collections.defaultdict(lambda: collections.defaultdict(list))
    skipped_big = skipped_net = skipped_volatile = 0
    for case_id, case in cases.items():
        if case_id not in legado or case_id not in rust:
            continue
        content = case.get("content") or ""
        if not content or len(content) > args.max_content:
            skipped_big += 1
            continue
        rule = (case.get("rule") or "").replace("\u00a0", " ")
        if any(
            mark in rule
            for mark in ("Date.now", "Date.parse", "currentTimeMillis", "new Date(")
        ):
            skipped_volatile += 1
            continue
        if "Jsoup.connect(" in rule or "java.ajax(" in rule:
            if "Jsoup.connect(" in rule:
                skipped_net += 1
                continue
        buckets[status(legado[case_id], rust[case_id])][case_id.split(":")[0]].append(case_id)

    picked: list[str] = []
    per_source: collections.Counter[str] = collections.Counter()
    total = 0

    def take(case_id: str) -> bool:
        nonlocal total
        size = len(cases[case_id].get("content") or "")
        if per_source[case_id.split(":")[0]] >= args.per_source:
            return False
        if len(picked) >= args.cases or total + size > args.max_total:
            return False
        picked.append(case_id)
        per_source[case_id.split(":")[0]] += 1
        total += size
        return True

    for state in STATUS_ORDER:
        by_source = buckets.get(state, {})
        cursor = {source: 0 for source in by_source}
        while True:
            progressed = False
            for source, ids in sorted(by_source.items()):
                while cursor[source] < len(ids):
                    case_id = ids[cursor[source]]
                    cursor[source] += 1
                    if take(case_id):
                        progressed = True
                        break
            if not progressed:
                break

    if not picked:
        print("❌ 一条都没挑到（先跑 scripts/ab/full_ab.sh？）", file=sys.stderr)
        return 1

    os.makedirs(args.out, exist_ok=True)
    with open(os.path.join(args.out, "cases.jsonl"), "w", encoding="utf-8") as handle:
        for case_id in picked:
            handle.write(json.dumps(cases[case_id], ensure_ascii=False) + "\n")
    with open(os.path.join(args.out, "legado.jsonl"), "w", encoding="utf-8") as handle:
        for case_id in picked:
            handle.write(json.dumps(legado[case_id], ensure_ascii=False) + "\n")

    counts = collections.Counter(status(legado[i], rust[i]) for i in picked)
    print(
        f"收录 {len(picked)} 条 / {len(per_source)} 个源 / content {total / 1024:.0f}KB "
        f"→ {args.out}（跳过：过大 {skipped_big}、Jsoup.connect {skipped_net}、时间/随机 {skipped_volatile}）"
    )
    print("状态分布:", dict(counts.most_common()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
