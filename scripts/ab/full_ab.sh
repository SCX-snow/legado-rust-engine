#!/usr/bin/env bash

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root" || exit 1
source scripts/cargo-env.sh

out="target/legado-ab-full"
limit="${1:-40}"
keyword="${2:-斗破苍穹}"
mkdir -p "$out"

if [ ! -x "target/legado-ab/toolchain/jdk/bin/java" ]; then
    echo "❌ 没有 harness 工具链：先跑 scripts/ab/fetch_toolchain.sh && scripts/ab/build_harness.sh" >&2
    exit 1
fi

if [ ! -s "$out/sources.json" ] || [ -n "${FETCH_SOURCES:-}" ]; then
    echo "== 1/5 抓书源合集 =="
    python3 scripts/ab/fetch_sources.py --out "$out/sources.json" || exit 1
fi

echo "== 2/5 真实站点抓页 → 对拍请求行（limit=$limit）=="
rm -f "$out/cases.jsonl" "$out/cases_search.jsonl" "$out/cases_chain.jsonl"
cargo run -q --example net_fetch -- "$limit" "$keyword" \
    --sources "$out/sources.json" --emit-cases "$out/cases_search.jsonl" || true
cargo run -q --example book_fetch -- "$limit" "$keyword" \
    --sources "$out/sources.json" --emit-cases "$out/cases_chain.jsonl" || true
for part in cases_search.jsonl cases_chain.jsonl; do
    [ -s "$out/$part" ] && cat "$out/$part" >>"$out/cases.jsonl"
done
if [ ! -s "$out/cases.jsonl" ]; then
    echo "❌ 一条用例都没生成（网络？limit 太小？）" >&2
    exit 1
fi
echo "请求行: $(wc -l <"$out/cases.jsonl") 条（搜索 $(wc -l <"$out/cases_search.jsonl" 2>/dev/null || echo 0) + 链路 $(wc -l <"$out/cases_chain.jsonl" 2>/dev/null || echo 0)）"

if [ ! -s "$out/legado_out.jsonl" ] || [ -n "${FORCE_LEGADO:-}" ]; then
    echo "== 3/5 legado（ICU 真机语义）录 oracle =="
    ICU_SHIM=1 scripts/ab/harness/run.sh <"$out/cases.jsonl" >"$out/legado_out.jsonl" || exit 1
fi

echo "== 4/5 我们跑一遍 =="
if [ -n "${AB_OFFLINE:-}" ]; then
    cargo run -q --example legado_ab -- --stdio <"$out/cases.jsonl" >"$out/rust_out.jsonl" || exit 1
else
    AB_HTTP=1 cargo run -q --example legado_ab -- --stdio <"$out/cases.jsonl" >"$out/rust_out.jsonl" || exit 1
fi

echo "== 5/5 汇总 =="
python3 scripts/ab/compare_oracles.py \
    --cases "$out/cases.jsonl" --a "$out/legado_out.jsonl" --b "$out/rust_out.jsonl"
