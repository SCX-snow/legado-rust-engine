#!/usr/bin/env bash

set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root" || exit 1
source scripts/cargo-env.sh

limit="${1:-60}"
log_dir="target/book-stats"
log="$log_dir/$(date +%F).log"
mkdir -p "$log_dir"

{
    printf '\n===== %s（limit=%s）=====\n' "$(date '+%F %T')" "$limit"
} >>"$log"

cargo run -q --example book_fetch -- "$limit" 2>&1 | tee -a "$log" | tail -3
exit "${PIPESTATUS[0]}"
