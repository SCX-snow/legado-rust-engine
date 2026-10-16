#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

mode="icu"
if [ "${1:-}" = "--desktop" ]; then
    mode="desktop"
fi

source scripts/cargo-env.sh

if [ ! -f target/legado-ab/harness/out/legado-rule.jar ]; then
    echo "❌ harness 未编译：先跑 scripts/ab/build_harness.sh replace" >&2
    exit 1
fi

echo "· 生成 cases"
python3 scripts/ab/gen_replace_cases.py

if [ "$mode" = "desktop" ]; then
    echo "· legado 侧（桌面 Java，对照）"
    bash scripts/ab/harness/run_replace.sh \
        < fixtures/replace_ab/cases.jsonl > fixtures/replace_ab/legado_desktop.jsonl
else
    echo "· legado 侧（ICU = 真机语义）"
    ICU_SHIM=1 bash scripts/ab/harness/run_replace.sh \
        < fixtures/replace_ab/cases.jsonl > fixtures/replace_ab/legado.jsonl
fi

echo "· Rust 侧（同时就是基线）"
cargo build -q --example legado_ab
./target/debug/examples/legado_ab --stdio \
    < fixtures/replace_ab/cases.jsonl > fixtures/replace_ab/rust.jsonl

echo "· 验证"
cargo run -q --example legado_ab -- --corpus fixtures/replace_ab
