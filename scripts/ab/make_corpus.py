#!/usr/bin/env python3
"""从 A/B 对拍产物里切一份**可提交、可重复**的比对语料。

输入是 legado JVM harness 与 Rust 驱动跑出来的四个文件（都在 `target/legado-ab/`，
`/target` 不进版本库，所以只有这份切片会跟着仓库走）：

    cases.jsonl       Rust / JVM 共用的请求行（`{"id","mode","rule","content","baseUrl"}`）
    legado_out.jsonl  legado（真实 Kotlin 引擎）的逐条结果
    rust_out.jsonl    Rust 引擎当时的逐条结果（只用来挑「有意思」的 case）
    page_cases.jsonl / page_legado.jsonl / page_rust.jsonl  真实抓取页面版（可选，体积大）

输出（默认 `fixtures/legado_ab/`）：

    cases.jsonl   挑中的请求行（去掉体积过大的）
    legado.jsonl  legado 的**实际行为**记录（`{"id","ok","result"|"error"}`）

判定用的基线 `rust.jsonl` 不在这里生成——要拿当前代码跑一遍 `examples/legado_ab.rs --emit`
再提交，保证「基线 = 当前行为」，之后任何漂移都会被 `legado_ab` 挡住。

用法：

    python3 scripts/ab/make_corpus.py                       # 合成语料，写 fixtures/legado_ab/
    python3 scripts/ab/make_corpus.py --with-pages 60       # 再混入 60 条小的真实页面用例
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
AB = os.path.join(REPO, "target", "legado-ab")
OUT = os.path.join(REPO, "fixtures", "legado_ab")

MAX_CONTENT = 4000
MAX_PAGE_CONTENT = 6000
CAPS = {
    "LEGADO_OK_RUST_ERR": 400,
    "DIFF_OK": 250,
    "RUST_OK_LEGADO_ERR": 120,
    "BOTH_ERR": 120,
    "SAME": 150,
}


def load(path):
    out = {}
    if not os.path.exists(path):
        return out
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            out[obj.get("id")] = obj
    return out


def norm(result):
    if isinstance(result, list):
        return tuple("" if item is None else str(item) for item in result)
    if result is None:
        return ("",)
    return (str(result),)


def status(legado, rust):
    if legado.get("ok") and rust.get("ok"):
        return "SAME" if norm(legado.get("result")) == norm(rust.get("result")) else "DIFF_OK"
    if legado.get("ok"):
        return "LEGADO_OK_RUST_ERR"
    if rust.get("ok"):
        return "RUST_OK_LEGADO_ERR"
    return "BOTH_ERR"


def cat(rule):
    tags = []
    if "@js:" in rule or "<js>" in rule:
        tags.append("js")
    if "##" in rule:
        tags.append("replace")
    if "||" in rule:
        tags.append("or")
    if "&&" in rule:
        tags.append("and")
    if "%%" in rule:
        tags.append("interleave")
    if "@xpath:" in rule or rule.startswith("/"):
        tags.append("xpath")
    if "@json:" in rule or rule.startswith("$.") or rule.startswith("$["):
        tags.append("json")
    if "@css:" in rule:
        tags.append("css")
    if "{{" in rule:
        tags.append("template")
    if "@get:" in rule:
        tags.append("get")
    if "@put:" in rule:
        tags.append("put")
    return ",".join(tags) or "plain"


def pick(cases, legado, rust, caps, max_content=MAX_CONTENT):
    """按 (mode, 类别) 分桶轮转取样，尽量铺开覆盖面。"""
    buckets = collections.defaultdict(list)
    for cid, case in cases.items():
        if len(case.get("content") or "") > max_content:
            continue
        lo, ro = legado.get(cid), rust.get(cid)
        if lo is None or ro is None:
            continue
        st = status(lo, ro)
        buckets[(st, case["mode"], cat(case["rule"]))].append(cid)

    chosen = []
    for st, cap in caps.items():
        keys = sorted(k for k in buckets if k[0] == st)
        taken = 0
        round_index = 0
        while taken < cap:
            progressed = False
            for key in keys:
                items = buckets[key]
                if round_index < len(items):
                    chosen.append(items[round_index])
                    taken += 1
                    progressed = True
                    if taken >= cap:
                        break
            if not progressed:
                break
            round_index += 1
        print(f"  {st:20} 取 {taken} 条", file=sys.stderr)
    return sorted(set(chosen))


def write(out_dir, ids, cases, legado):
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "cases.jsonl"), "w", encoding="utf-8") as cases_out, open(
        os.path.join(out_dir, "legado.jsonl"), "w", encoding="utf-8"
    ) as legado_out:
        for cid in ids:
            case = cases[cid]
            request = {
                k: case[k]
                for k in ("id", "mode", "rule", "content", "baseUrl", "source", "book", "chapter")
                if case.get(k) is not None
            }
            cases_out.write(json.dumps(request, ensure_ascii=False) + "\n")
            lo = legado[cid]
            record = {"id": cid, "ok": bool(lo.get("ok"))}
            if lo.get("ok"):
                record["result"] = lo.get("result")
            else:
                record["error"] = str(lo.get("error"))[:500]
            legado_out.write(json.dumps(record, ensure_ascii=False) + "\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ab-dir", default=AB)
    parser.add_argument("--out-dir", default=OUT)
    parser.add_argument(
        "--with-pages",
        type=int,
        default=200,
        help="再混入多少条真实抓取页面用例（默认 200，只挑小的；0 = 不混）",
    )
    args = parser.parse_args()

    cases = load(os.path.join(args.ab_dir, "cases.jsonl"))
    legado = load(os.path.join(args.ab_dir, "legado_out.jsonl"))
    rust = load(os.path.join(args.ab_dir, "rust_out.jsonl"))
    if not cases or not legado or not rust:
        print(
            f"{args.ab_dir} 里缺少对拍产物（cases/legado_out/rust_out 三个都要有）；"
            "先跑全量对拍生成",
            file=sys.stderr,
        )
        return 1

    print(f"合成语料 {len(cases)} 条，legado 结果 {len(legado)} 条", file=sys.stderr)
    ids = pick(cases, legado, rust, CAPS)

    if args.with_pages:
        pcases = load(os.path.join(args.ab_dir, "page_cases.jsonl"))
        plegado = load(os.path.join(args.ab_dir, "page_legado.jsonl"))
        prust = load(os.path.join(args.ab_dir, "page_rust.jsonl"))
        per_status = max(1, args.with_pages // 5)
        page_caps = {
            "LEGADO_OK_RUST_ERR": per_status,
            "DIFF_OK": per_status,
            "SAME": per_status,
            "RUST_OK_LEGADO_ERR": per_status,
            "BOTH_ERR": per_status,
        }
        existing = set(ids)
        pids = [
            i for i in pick(pcases, plegado, prust, page_caps, MAX_PAGE_CONTENT) if i not in existing
        ]
        cases.update(pcases)
        legado.update(plegado)
        ids += pids
        print(f"真实页面再取 {len(pids)} 条", file=sys.stderr)

    if not ids:
        print("一条都没挑出来，拒绝用空语料覆盖 fixtures/", file=sys.stderr)
        return 1

    write(args.out_dir, ids, cases, legado)
    total = sum(os.path.getsize(os.path.join(args.out_dir, f)) for f in ("cases.jsonl", "legado.jsonl"))
    print(f"写出 {len(ids)} 条到 {args.out_dir}（{total / 1e6:.1f} MB）", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
