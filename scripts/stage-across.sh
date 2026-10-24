#!/usr/bin/env bash
# Run one stage across several pages and watch the resident set, to find which
# stage leaks across pages.
# usage: stage-across.sh <stage> [pages]
#
# <stage> is the snake_case `Stage` value: detection | ocr | translation | inpainting.
# An invalid value makes the typed `process` call reject; this fails fast rather
# than counting pages that never ran.
#
# KOHARU_PACKAGE      Android package to drive (default rs.koharu.app)
# KOHARU_PROJECT_NAME UI project label to open first; empty keeps whatever
#                     project the app already has open (default empty)
# KOHARU_CDP_SCRIPT   CDP driver invoked per probe (default ./cdp.mjs beside us)
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
package=${KOHARU_PACKAGE:-rs.koharu.app}
project=${KOHARU_PROJECT_NAME:-}
cdp=${KOHARU_CDP_SCRIPT:-$here/cdp.mjs}

if ! command -v adb >/dev/null 2>&1; then
  for sdk in "${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}"; do
    [ -n "$sdk" ] && [ -x "$sdk/platform-tools/adb" ] || continue
    PATH="$sdk/platform-tools:$PATH"
    break
  done
fi
command -v adb >/dev/null 2>&1 || { echo "adb not found on PATH, ANDROID_HOME, or ANDROID_SDK_ROOT" >&2; exit 1; }

STAGE=${1:?stage}
N=${2:-4}

pss() { adb shell dumpsys meminfo "$package" 2>/dev/null | awk '/TOTAL/{print $2; exit}'; }

adb shell am force-stop "$package"
sleep 2
adb shell monkey -p "$package" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1
sleep 16
PID=$(adb shell pidof "$package" | tr -d '\r')
adb forward --remove-all >/dev/null 2>&1
adb forward tcp:9222 localabstract:webview_devtools_remote_"$PID" >/dev/null
sleep 1
if [ -n "$project" ]; then
  node "$cdp" "(()=>{const b=[...document.querySelectorAll(\"button,[role=button],a\")].find(e=>e.innerText.includes(\"$project\"));if(b)b.click();return \"opened\"})()" >/dev/null
  sleep 8
fi

echo "stage=$STAGE baseline=$(pss) KB"
for i in $(seq 0 $((N-1))); do
  ID=$(node "$cdp" "window.__TAURI_INTERNALS__.invoke('get_pages').then(p=>p[$i].id)" 2>/dev/null | tr -d '"' | tr -d '\r')
  [ -n "$ID" ] || { echo "page $((i + 1)): no page id (is a project open?)" >&2; exit 1; }
  node "$cdp" "window.__TAURI_INTERNALS__.invoke('process',{scope:{scope:'pages',value:['$ID']},operation:{operation:'only',stage:'$STAGE'}}).then(()=> 'ok')" >/dev/null ||
    { echo "page $((i + 1)): stage '$STAGE' rejected; valid stages are detection|ocr|translation|inpainting" >&2; exit 1; }
  for _ in $(seq 1 40); do
    sleep 5
    st=$(node "$cdp" "(()=>{const t=document.body.innerText;return /\\bStop\\b/.test(t)?'running':'idle'})()" 2>/dev/null | tr -d '"')
    [ "$st" = "idle" ] && break
  done
  echo "page $((i+1)): $(pss) KB"
done
echo "alive=$(adb shell pidof "$package")"
