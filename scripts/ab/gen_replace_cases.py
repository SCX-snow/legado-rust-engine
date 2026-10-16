#!/usr/bin/env python3
"""用 `fixtures/replace_rules.json`（miaogongzi jh.html 的真实净化规则）生成 A/B 语料。

判定的是「替换引擎 + 规则筛选」：每个 case 带一整套规则 + 一段文本 + 目标（标题/正文）
+ 书名/来源，两边各自跑：

    legado 侧：scripts/ab/harness/run_replace.sh（ICU_SHIM=1 为真机语义）
    Rust 侧：  ./target/debug/examples/legado_ab --stdio

生成 `fixtures/replace_ab/cases.jsonl`（提交），再由 `scripts/ab/regen_replace.sh` 补上
`legado.jsonl` / `rust.jsonl`。

真实规则本身不带 scope / excludeScope，为了把筛选语义也覆盖到，**派生**了两条改过 scope 的
规则（名字加「(派生)」后缀），只出现在部分 case 里。
"""

from __future__ import annotations

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
OUT_DIR = os.path.join(REPO, "fixtures", "replace_ab")
RULES = os.path.join(REPO, "fixtures", "replace_rules.json")

CHAPTER = (
    "第１２３章　测试章节\n"
    "\n"
    "\u200b这是一段“测试”文字（括号）——破折号……省略号，以及网址 http://a.com/x 和 null 。\n"
    "\n"
    "ＡＢＣ１２３＜／＞｛｝全角字符也要转过来。\n"
    "\n"
    "【书名】『引号』「括号」〈尖括号〉都要处理。\n"
    "\n"
    "作者的话：这一章写得有点长，请见谅。\n"
    "\n"
    "检测到你的最新阅读进度为“第122章”，是否同步？\n"
    "\n"
    "重复段落。\n"
    "重复段落。\n"
    "\n"
    "（本章完）\n"
)

TITLE = "第１２３章　测试“标题”【上】"

COMBOS = [
    ("content", "斗破苍穹", "https://www.a.com", False),
    ("content", "别的书", "https://other.com", False),
    ("title", "斗破苍穹", "https://www.a.com", False),
    ("title", "别的书", "https://other.com", False),
    ("content", "斗破苍穹", "https://www.a.com", True),
    ("content", "别的书", "https://other.com", True),
    ("title", "斗破苍穹", "https://www.a.com", True),
    ("title", "别的书", "https://other.com", True),
]


def derived(rules, *, scope=None, exclude=None, suffix):
    out = []
    for index in (13, 14):
        base = dict(rules[index])
        base["name"] = f"{base.get('name', '')}{suffix}"
        if scope is not None:
            base["scope"] = scope
        if exclude is not None:
            base["excludeScope"] = exclude
        out.append(base)
    return out


def main():
    rules = json.load(open(RULES, encoding="utf-8"))
    scoped = derived(rules, scope="斗破苍穹", suffix="(派生 scope)")
    excluded = derived(rules, exclude="斗破苍穹", suffix="(派生 exclude)")

    cases = []
    for target, book_name, origin, use_derived in COMBOS:
        picked = list(rules)
        if use_derived:
            picked += scoped + excluded
        cases.append(
            {
                "id": f"replace:{target}:{'scope' if use_derived else 'plain'}:{book_name}",
                "kind": "replace",
                "target": target,
                "text": CHAPTER if target == "content" else TITLE,
                "bookName": book_name,
                "bookOrigin": origin,
                "rules": picked,
            }
        )
    for index, rule in enumerate(rules):
        for target in ("content", "title"):
            cases.append(
                {
                    "id": f"replace:rule{index:02d}:{rule.get('name', '')}:{target}",
                    "kind": "replace",
                    "target": target,
                    "text": CHAPTER if target == "content" else TITLE,
                    "bookName": "斗破苍穹",
                    "bookOrigin": "https://www.a.com",
                    "rules": [rule],
                }
            )

    os.makedirs(OUT_DIR, exist_ok=True)
    path = os.path.join(OUT_DIR, "cases.jsonl")
    with open(path, "w", encoding="utf-8") as handle:
        for case in cases:
            handle.write(json.dumps(case, ensure_ascii=False) + "\n")
    print(f"写出 {len(cases)} 条到 {path}（{os.path.getsize(path)/1e6:.1f} MB）", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
