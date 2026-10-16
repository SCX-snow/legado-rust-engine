#!/usr/bin/env bash
set -e
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
T="$root/target/legado-ab/toolchain"
OUT="$root/target/legado-ab/harness/out"
CP="$OUT/legado-rule.jar:$T/kotlinc/lib/kotlin-stdlib.jar:$(ls "$T"/jars/*.jar | tr '\n' ':')"

PATCH=()
if [ -n "${ICU_SHIM:-}" ]; then
    SHIM="$root/target/legado-ab/icu-shim"
    PATCH=(--patch-module "java.base=$SHIM/classes" "-Dlegado.icu.bridge=$SHIM/libicu_bridge.so")
fi

exec "$T/jdk/bin/java" "${PATCH[@]}" -Dfile.encoding=UTF-8 -cp "$CP" cli.MainKt
