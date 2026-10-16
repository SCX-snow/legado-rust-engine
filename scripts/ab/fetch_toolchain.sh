#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
T="$root/target/legado-ab/toolchain"
mkdir -p "$T/jars"

fetch() {
    local url="$1" out="$2"
    if [ -s "$out" ]; then echo "· 已有 $(basename "$out")"; return; fi
    echo "· 下载 $(basename "$out")"
    curl -sSL --fail --retry 3 --max-time 600 -o "$out.part" "$url"
    mv "$out.part" "$out"
}

if [ ! -x "$T/jdk/bin/java" ]; then
    fetch "https://api.adoptium.net/v3/binary/latest/17/ga/linux/x64/jdk/hotspot/normal/eclipse" "$T/jdk.tar.gz"
    mkdir -p "$T/jdk" && tar -xzf "$T/jdk.tar.gz" -C "$T/jdk" --strip-components=1
fi
echo "· JDK: $("$T/jdk/bin/java" -version 2>&1 | head -1)"

if [ ! -x "$T/kotlinc/bin/kotlinc" ]; then
    fetch "https://github.com/JetBrains/kotlin/releases/download/v2.0.21/kotlin-compiler-2.0.21.zip" "$T/kotlinc.zip"
    mkdir -p "$T/kotlinc" && unzip -q -o "$T/kotlinc.zip" -d "$T"
fi
echo "· kotlinc: $("$T/kotlinc/bin/kotlinc" -version 2>&1 | head -1)"

M="https://repo1.maven.org/maven2"
fetch "$M/org/jsoup/jsoup/1.16.2/jsoup-1.16.2.jar"                       "$T/jars/jsoup-1.16.2.jar"
fetch "$M/com/jayway/jsonpath/json-path/2.9.0/json-path-2.9.0.jar"      "$T/jars/json-path-2.9.0.jar"
fetch "$M/net/minidev/json-smart/2.5.0/json-smart-2.5.0.jar"            "$T/jars/json-smart-2.5.0.jar"
fetch "$M/net/minidev/accessors-smart/2.5.0/accessors-smart-2.5.0.jar"  "$T/jars/accessors-smart-2.5.0.jar"
fetch "$M/org/ow2/asm/asm/9.6/asm-9.6.jar"                              "$T/jars/asm-9.6.jar"
fetch "$M/org/slf4j/slf4j-api/2.0.9/slf4j-api-2.0.9.jar"                "$T/jars/slf4j-api-2.0.9.jar"
fetch "$M/cn/wanghaomiao/JsoupXpath/2.5.3/JsoupXpath-2.5.3.jar"         "$T/jars/JsoupXpath-2.5.3.jar"
fetch "$M/org/antlr/antlr4-runtime/4.7.2/antlr4-runtime-4.7.2.jar"      "$T/jars/antlr4-runtime-4.7.2.jar"
fetch "$M/org/apache/commons/commons-text/1.10.0/commons-text-1.10.0.jar" "$T/jars/commons-text-1.10.0.jar"
fetch "$M/org/apache/commons/commons-lang3/3.12.0/commons-lang3-3.12.0.jar" "$T/jars/commons-lang3-3.12.0.jar"
fetch "$M/com/google/code/gson/gson/2.10.1/gson-2.10.1.jar"             "$T/jars/gson-2.10.1.jar"
fetch "$M/org/jetbrains/kotlinx/kotlinx-coroutines-core-jvm/1.7.3/kotlinx-coroutines-core-jvm-1.7.3.jar" "$T/jars/kotlinx-coroutines-core-jvm-1.7.3.jar"
LEGADO_REPO="${LEGADO_REPO:-$HOME/workspace/Java/legado}"
if [ -f "$LEGADO_REPO/modules/rhino/lib/rhino-1.7.14.jar" ]; then
    [ -s "$T/jars/rhino-1.7.14.jar" ] || cp "$LEGADO_REPO/modules/rhino/lib/rhino-1.7.14.jar" "$T/jars/"
    echo "· rhino 取自 legado 仓库"
else
    fetch "$M/org/mozilla/rhino/1.7.14/rhino-1.7.14.jar"                "$T/jars/rhino-1.7.14.jar"
fi

echo
echo "✅ 工具链就绪：$T"
ls "$T/jars" | sed 's/^/   /'
