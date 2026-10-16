#!/usr/bin/env bash

set -uo pipefail

_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$_repo_root" || exit 1
source scripts/cargo-env.sh

offline=(
    rule_eval
    rule_corpus
    roundtrip_sources
    model_basics
    js_eval
    query_ttf
    net_offline
    book_offline
    replace_eval
    rss_offline
    perf
    legado_ab
)

failed=0
for ex in "${offline[@]}"; do
    printf '\n===== %s =====\n' "$ex"
    if ! cargo run -q --example "$ex"; then
        echo "❌ $ex 失败"
        failed=1
    fi
done

printf '\n===== cargo fmt --all --check =====\n'
cargo fmt --all --check || {
    echo "❌ 格式没排好：跑 cargo fmt --all"
    failed=1
}

printf '\n===== cargo clippy --workspace --all-targets =====\n'
cargo clippy --workspace --all-targets || {
    echo "❌ clippy 不干净"
    failed=1
}

printf '\n===== cargo clippy --workspace --all-targets --features redis =====\n'
cargo clippy --workspace --all-targets --features redis || {
    echo "❌ clippy（redis feature）不干净"
    failed=1
}

if [ "$failed" -ne 0 ]; then
    echo
    echo "❌ 离线验收有没过的项"
    exit 1
fi

echo
echo "✅ 离线验收全部通过"

if [ "${1:-}" = "--online" ]; then
    printf '\n===== 联网验收（失败只提醒，不否决）=====\n'
    for ex in net_fetch book_fetch rss_fetch; do
        printf '\n----- %s -----\n' "$ex"
        cargo run -q --example "$ex" || echo "⚠ $ex 没通过（网络不可控）"
    done
    printf '\n----- cache_redis（要真 Redis）-----\n'
    cargo run -q --example cache_redis --features redis \
        || echo "⚠ cache_redis 没通过（本机没有 Redis？）"
fi
