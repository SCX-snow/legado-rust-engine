#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-all}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LEGADO_REPO="${LEGADO_REPO:-$HOME/workspace/Java/legado}"
T="$root/target/legado-ab/toolchain"
H="$root/target/legado-ab/harness"
SRC="$H/src"
KOTLINC="$T/kotlinc/bin/kotlinc"

[ -x "$T/jdk/bin/java" ] || { echo "❌ 先跑 scripts/ab/fetch_toolchain.sh" >&2; exit 1; }
[ -x "$KOTLINC" ]        || { echo "❌ 先跑 scripts/ab/fetch_toolchain.sh" >&2; exit 1; }

echo "· 组装源码到 $SRC"
rm -rf "$SRC"
mkdir -p "$SRC"
cp -r "$root/scripts/ab/harness/src/." "$SRC/"
copy() {
    local rel="$1"
    [ -e "$LEGADO_REPO/$rel" ] || { echo "   ⚠ 缺 $rel"; return; }
    mkdir -p "$SRC/legado/$(dirname "$rel")"
    cp -r "$LEGADO_REPO/$rel" "$SRC/legado/$rel"
}
if [ "$MODE" = "replace" ]; then
    rm -rf "$SRC/stub/io" "$SRC/stub/splitties" "$SRC/cli/Main.kt"
else
    for f in \
        app/src/main/java/io/legado/app/model/analyzeRule/AnalyzeRule.kt \
        app/src/main/java/io/legado/app/model/analyzeRule/AnalyzeByJSoup.kt \
        app/src/main/java/io/legado/app/model/analyzeRule/AnalyzeByJSonPath.kt \
        app/src/main/java/io/legado/app/model/analyzeRule/AnalyzeByRegex.kt \
        app/src/main/java/io/legado/app/model/analyzeRule/AnalyzeByXPath.kt \
        app/src/main/java/io/legado/app/model/analyzeRule/RuleAnalyzer.kt \
        app/src/main/java/io/legado/app/model/analyzeRule/RuleData.kt \
        app/src/main/java/io/legado/app/model/analyzeRule/RuleDataInterface.kt \
        app/src/main/java/io/legado/app/constant/AppPattern.kt \
        app/src/main/java/io/legado/app/constant/BookSourceType.kt \
        app/src/main/java/io/legado/app/constant/BookType.kt \
        app/src/main/java/io/legado/app/data/entities/BookSource.kt \
        app/src/main/java/io/legado/app/data/entities/rule \
        app/src/main/java/io/legado/app/utils/EncoderUtils.kt \
        app/src/main/java/io/legado/app/utils/HtmlFormatter.kt \
    ; do
        copy "$f"
    done
fi
copy modules/rhino/src/main/java/com/script

echo "· kotlinc 编译"
rm -rf "$H/out" && mkdir -p "$H/out"
export JAVA_HOME="$T/jdk"
export PATH="$T/jdk/bin:$PATH"
JARS="$(ls "$T"/jars/*.jar | tr '\n' ':')"
set +e
LOG="$H/kotlinc.log"
"$KOTLINC" -nowarn -jvm-target 17 \
    -classpath "$JARS" \
    -d "$H/out/legado-rule.jar" \
    $(find "$SRC" -name '*.kt' -o -name '*.java' | tr '\n' ' ') > "$LOG" 2>&1
status=$?
set -e
tail -40 "$LOG"
if [ "$status" -ne 0 ]; then
    echo
    echo "❌ kotlinc 失败（退出码 $status），完整日志：$LOG"
    exit "$status"
fi

echo
echo "✅ 编译完成：$H/out/legado-rule.jar"
