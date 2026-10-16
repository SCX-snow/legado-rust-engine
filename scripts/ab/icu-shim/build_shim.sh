#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SHIM="$root/scripts/ab/icu-shim"
T="$root/target/legado-ab/toolchain"
OUT="$root/target/legado-ab/icu-shim"
mkdir -p "$OUT/classes"

echo "· 编译 JNI 桥"
gcc -shared -fPIC -O1 -o "$OUT/libicu_bridge.so" "$SHIM/icu_bridge.c" \
    -I"$T/jdk/include" -I"$T/jdk/include/linux" \
    -I/usr/include -L/usr/lib64 -licui18n -licuuc

echo "· 编译 java.util.regex 垫片"
"$T/jdk/bin/javac" --patch-module java.base="$SHIM" -nowarn \
    -d "$OUT/classes" \
    "$SHIM/java/util/regex/Pattern.java" \
    "$SHIM/java/util/regex/Matcher.java" \
    "$SHIM/java/util/regex/IcuBridge.java"

echo
echo "✅ 垫片就绪：$OUT"
echo "   用法：ICU_SHIM=1 scripts/ab/harness/run_replace.sh < cases.jsonl"
