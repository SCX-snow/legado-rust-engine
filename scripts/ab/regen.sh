#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

AB="target/legado-ab"
if [ ! -d "$AB/harness" ]; then
    echo "❌ $AB/harness 不存在：先跑 scripts/ab/fetch_toolchain.sh && scripts/ab/build_harness.sh" >&2
    exit 1
fi
if [ ! -s "$AB/cases.jsonl" ]; then
    echo "❌ $AB/cases.jsonl 不存在（整批对拍的请求行）：先跑全量对拍生成全量语料" >&2
    exit 1
fi

source scripts/cargo-env.sh

if [ -f "$AB/legado_out.jsonl" ] && [ -z "${FORCE_LEGADO:-}" ]; then
    echo "· 复用已有 $AB/legado_out.jsonl（要重跑 legado 侧就 FORCE_LEGADO=1）"
else
    echo "· legado 侧：harness 跑 $(wc -l < "$AB/cases.jsonl") 条（约 12 分钟）"
    bash scripts/ab/harness/run.sh < "$AB/cases.jsonl" > "$AB/legado_out.jsonl"
fi

echo "· Rust 侧：legado_ab --stdio"
cargo build -q --example legado_ab
"./target/debug/examples/legado_ab" --stdio < "$AB/cases.jsonl" > "$AB/rust_out.jsonl"

echo "· 切语料"
python3 scripts/ab/make_corpus.py

echo "· 并入手工整理的判别 case（D11/D16/D21/D22，见 scripts/ab/curated_cases.jsonl）"
ICU_SHIM=1 bash scripts/ab/harness/run.sh \
    < scripts/ab/curated_cases.jsonl > "$AB/curated_legado.jsonl"
python3 scripts/ab/merge_curated.py "$AB/curated_legado.jsonl"
echo "  ⚠ 记得把 examples/legado_ab.rs 的 EXPECTED_CASES 改成语料实际条数"

echo "· 重新基线（当前行为）"
cargo run -q --example legado_ab -- --emit fixtures/legado_ab/rust.jsonl >/dev/null

echo
echo "✅ 语料与基线已更新，review 一下 fixtures/legado_ab/ 的 diff 再提交"
cargo run -q --example legado_ab
