#!/bin/bash
# Rebuild the frontend, serve it with a mocked Tauri backend (mock.js), and screenshot screens
# with headless Chrome over the DevTools protocol (cdp.mjs).
#
# usage: scripts/screenshots/reshoot.sh                      (all screens)
#        scripts/screenshots/reshoot.sh "name|WxH|?query" ...  (selected screens)
#
# Output: scripts/screenshots/out/shots/<name>.png (git-ignored).
# Environment:
#   CHROME     path of the Chrome binary (default: /Applications/Google Chrome.app/...)
#   PORT       port of the static server (default 4789)
#   CDP_PORT   Chrome remote debugging port (default 9333)
#   WAIT       ms to wait after each navigation (default 3500, read by cdp.mjs)
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP="$(cd "$DIR/../../app" && pwd)"
OUT="$DIR/out"
SITE="$OUT/site"
SHOTS="$OUT/shots"
PROFILE="$OUT/chrome-profile"
CH="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
PORT="${PORT:-4789}"
CDP_PORT="${CDP_PORT:-9333}"

[ -x "$CH" ] || { echo "Chrome not found: $CH (set CHROME)"; exit 1; }
if nc -z 127.0.0.1 "$PORT" 2>/dev/null; then echo "port $PORT is in use (set PORT)"; exit 1; fi

(cd "$APP" && npm run build >/dev/null 2>&1) || { echo "build failed (run npm run build in app/)"; exit 1; }
mkdir -p "$SITE" "$SHOTS" "$PROFILE"
find "$SITE" -mindepth 1 -delete
cp -R "$APP/dist/." "$SITE/"
cp "$DIR/mock.js" "$SITE/mock.js"
python3 -c "import sys; p=sys.argv[1]; s=open(p).read(); open(p,'w').write(s.replace('<head>','<head><script src=\"/mock.js\"></script>',1))" "$SITE/index.html"

SERVER_PID=""
CHROME_PID=""
cleanup() {
  [ -n "$CHROME_PID" ] && kill "$CHROME_PID" 2>/dev/null || true
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
}
trap cleanup EXIT

(cd "$SITE" && exec python3 -m http.server "$PORT" --bind 127.0.0.1 >/dev/null 2>&1) &
SERVER_PID=$!
"$CH" --headless=new --disable-gpu --hide-scrollbars --user-data-dir="$PROFILE" \
  --remote-debugging-port="$CDP_PORT" about:blank >/dev/null 2>&1 &
CHROME_PID=$!
for _ in $(seq 1 40); do
  curl -s "http://127.0.0.1:$CDP_PORT/json/version" >/dev/null && nc -z 127.0.0.1 "$PORT" && break
  sleep 0.5
done

if [ $# -eq 0 ]; then set -- \
 "digest-light|1200x800|?x=1" "digest-dark|1200x800|?dark=1" "feed-light|1200x800|?go=フィード" "feed-dark|1200x800|?dark=1&go=フィード" \
 "first|1200x800|?s=first&go=フィード" "sources|1200x800|?go=設定" "sources-confirm|1200x800|?go=設定,Zenn を削除" "sources-edit|1200x800|?go=設定,Zenn を編集" \
 "rss|1200x800|?go=設定,RSSを追加" "topics-confirm|1200x800|?go=設定,トピック,認知症 を削除" "topics-edit|1200x800|?go=設定,トピック,認知症 を編集" \
 "llm|1200x1000|?go=設定,LLM" "schedule|1200x1000|?go=設定,スケジュール" "feed-small|960x640|?go=フィード" "digest-small|960x640|?x=1" \
 "sources-small|960x640|?go=設定,Zenn を削除" "topics-small-dark|960x640|?dark=1&go=設定,トピック,認知症 を削除"; fi
BASE="http://127.0.0.1:$PORT" node "$DIR/cdp.mjs" "$CDP_PORT" "$SHOTS" "$@"
