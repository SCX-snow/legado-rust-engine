#!/usr/bin/env python3
"""逐条对比两份 legado 结果文件（A/B 对拍产物）并按规则特征归类差异。

两份都是 `{"id","ok","result"|"error"}` 的 JSONL（legado harness 的输出格式），
`--cases` 是请求行（拿 rule 文本做特征归类）。

    python3 scripts/ab/compare_oracles.py \
        --cases fixtures/legado_ab/cases.jsonl \
        --a fixtures/legado_ab/legado.jsonl \
        --b target/legado-ab/legado_out_desktop_now.jsonl

输出：
  - 分布：SAME / DIFF_OK（两边都成功但值不同）/ A_OK_B_ERR / A_ERR_B_OK / BOTH_ERR / MISSING；
  - 每个 DIFF 的规则特征（`\\w \\d \\s \\b (?i) \\h`、`@js:`、xpath、json、css、`##`、`%%`…）；
  - `--samples N` 条可读样本（默认 10）；
  - `--only-feature` 只看带某特征的 case。
"""

from __future__ import annotations

import argparse
import collections
import json
import re
import sys

FEATURES = [
    ("backslash-w", re.compile(r"\\w")),
    ("backslash-d", re.compile(r"\\d")),
    ("backslash-s", re.compile(r"\\s")),
    ("backslash-b", re.compile(r"\\b")),
    ("case-insensitive", re.compile(r"\(\?i\)")),
    ("hex-h", re.compile(r"\\h")),
    ("js", re.compile(r"@js:|<js>")),
    ("js-braces", re.compile(r"\{\{")),
    ("replace-##", re.compile(r"##")),
    ("or-||", re.compile(r"\|\|")),
    ("and-&&", re.compile(r"&&")),
    ("interleave-%%", re.compile(r"%%")),
    ("xpath", re.compile(r"@xpath:|@XPath:|/html|^//|^\$\.\.")),
    ("json", re.compile(r"@json:|@Json:|\$\.")),
    ("css", re.compile(r"@css:|@CSS:")),
]


def load(path: str) -> dict:
    out = {}
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            obj = json.loads(line)
            out[obj["id"]] = obj
    return out


def canon(out: dict):
    """只看语义输出：错误文案不参与比较（两边的文案会随版本/栈帧变）"""
    if not out.get("ok"):
        return None
    result = out.get("result")
    if isinstance(result, list):
        return tuple("" if item is None else str(item) for item in result)
    if result is None:
        return ("",)
    return (str(result),)


def status(a: dict, b: dict) -> str:
    ka, kb = canon(a), canon(b)
    if a.get("ok") and b.get("ok"):
        return "SAME" if ka == kb else "DIFF_OK"
    if a.get("ok"):
        return "A_OK_B_ERR"
    if b.get("ok"):
        return "A_ERR_B_OK"
    return "BOTH_ERR"


def features(rule: str) -> list:
    return [name for name, pattern in FEATURES if pattern.search(rule)]


def brief(text, limit=120):
    if text is None:
        return ""
    text = str(text)
    return text if len(text) <= limit else text[:limit] + f"…(+{len(text) - limit})"


def show(out: dict, limit=160):
    if out.get("ok"):
        return "ok " + brief(json.dumps(out.get("result"), ensure_ascii=False), limit)
    return "err " + brief(out.get("error"), limit)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cases", required=True)
    parser.add_argument("--a", required=True, help="基准结果（例如旧 oracle / 桌面模式）")
    parser.add_argument("--b", required=True, help="要比较的结果（例如本轮 / ICU 模式）")
    parser.add_argument("--samples", type=int, default=10)
    parser.add_argument("--only-feature", default=None)
    parser.add_argument("--max-lines", type=int, default=0, help="最多打印多少条 diff（0=全部）")
    args = parser.parse_args()

    cases = {obj["id"]: obj for obj in (json.loads(l) for l in open(args.cases, encoding="utf-8") if l.strip())}
    a, b = load(args.a), load(args.b)

    counts = collections.Counter()
    feature_counts = collections.defaultdict(collections.Counter)
    diffs = []
    for cid, case in cases.items():
        if cid not in a or cid not in b:
            counts["MISSING"] += 1
            continue
        st = status(a[cid], b[cid])
        counts[st] += 1
        feats = tuple(features(case.get("rule") or "")) or ("plain",)
        for feat in feats:
            feature_counts[feat][st] += 1
        if st == "SAME":
            continue
        if args.only_feature and args.only_feature not in feats:
            continue
        diffs.append((cid, case, a[cid], b[cid], st, feats))

    total = sum(counts.values())
    print(f"cases {len(cases)} 条；参与比较 {total} 条（MISSING = 缺结果 {counts['MISSING']}）")
    print(f"A = {args.a}")
    print(f"B = {args.b}")
    for key in ["SAME", "DIFF_OK", "A_OK_B_ERR", "A_ERR_B_OK", "BOTH_ERR"]:
        count = counts[key]
        ratio = 0.0 if total == 0 else 100.0 * count / total
        print(f"  {key:12} {count:5}  ({ratio:5.1f}%)")

    if feature_counts:
        print("\n按规则特征（只看非 SAME 的占比；同一 case 可命中多个特征）")
        for feat, counter in sorted(feature_counts.items(), key=lambda kv: -sum(kv[1].values())):
            changed = sum(v for k, v in counter.items() if k != "SAME")
            allv = sum(counter.values())
            print(f"  {feat:18} 共 {allv:5}  变了 {changed:5}  ({100.0 * changed / allv:5.1f}%)")

    print(f"\n非 SAME 共 {len(diffs)} 条" + (f"（--only-feature {args.only_feature} 过滤后）" if args.only_feature else ""))
    limit = args.samples if args.samples and not args.max_lines else args.max_lines or args.samples
    shown = diffs[: limit if limit else len(diffs)]
    for cid, case, oa, ob, st, feats in shown:
        print(f"\n--- [{st}] {cid} mode={case.get('mode')} 特征={','.join(feats)}")
        print(f"    rule: {brief(case.get('rule'), 200)}")
        print(f"    A: {show(oa)}")
        print(f"    B: {show(ob)}")
    if limit and len(diffs) > limit:
        print(f"\n…… 还有 {len(diffs) - limit} 条（--samples/--max-lines 控制）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
