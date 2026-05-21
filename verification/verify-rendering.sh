#!/usr/bin/env bash
set -euo pipefail

# This flow verifies rendering behavior with screenshots.
# It complements verify-navigation.sh and intentionally does not target TLA validation.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${FORMAL_WEB_WEBDRIVER_PORT:-4451}"
STARTUP_URL="file://${ROOT}/artifacts/StartupExample.html"
HEADLESS_VIEWPORT_HEIGHT=600
SCREENSHOT_CHECKER="$ROOT/target/release/webdriver-screenshot-check"

if [[ -n "${FORMAL_WEB_VERIFY_WORK_DIR:-}" ]]; then
    WORK_DIR="$FORMAL_WEB_VERIFY_WORK_DIR"
    WORK_DIR_CREATED=0
    mkdir -p "$WORK_DIR"
else
    WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/formal-web-verify-rendering.XXXXXX")"
    WORK_DIR_CREATED=1
fi

if [[ -n "${FORMAL_WEB_VERIFY_LOG_FILE:-}" ]]; then
    LOG_FILE="$FORMAL_WEB_VERIFY_LOG_FILE"
    LOG_FILE_CREATED=0
else
    LOG_FILE="$WORK_DIR/verify.log"
    LOG_FILE_CREATED=1
fi

FORMAL_WEB_PID=""
SESSION_ID=""

cleanup() {
    local exit_code=$?
    if [[ -n "$SESSION_ID" ]]; then
        curl --silent --show-error -X DELETE "http://127.0.0.1:${PORT}/session/${SESSION_ID}" >/dev/null 2>&1 || true
    fi
    if [[ -n "$FORMAL_WEB_PID" ]] && kill -0 "$FORMAL_WEB_PID" 2>/dev/null; then
        kill "$FORMAL_WEB_PID" 2>/dev/null || true
        wait "$FORMAL_WEB_PID" 2>/dev/null || true
    fi
    if [[ $exit_code -eq 0 ]]; then
        if [[ $LOG_FILE_CREATED -eq 1 ]]; then
            rm -f "$LOG_FILE"
        fi
        if [[ $WORK_DIR_CREATED -eq 1 ]]; then
            rm -rf "$WORK_DIR"
        fi
    else
        echo "verification log: $LOG_FILE" >&2
        echo "verification artifacts: $WORK_DIR" >&2
    fi
}
trap cleanup EXIT

fail_with_log() {
    echo "$1" >&2
    if [[ -f "$LOG_FILE" ]]; then
        tail -n 50 "$LOG_FILE" >&2 || true
    fi
    exit 1
}

json_value() {
    local path="$1"
    python3 -c '
import json
import sys

raw = sys.stdin.read()
path_arg = sys.argv[1]
try:
    data = json.loads(raw)
except Exception as exc:
    raise SystemExit(f"json_value({path_arg}) failed to parse input: {exc}; input={raw[:400]!r}")

path = [part for part in path_arg.split(".") if part]
try:
    for part in path:
        if isinstance(data, list):
            data = data[int(part)]
        else:
            data = data[part]
except Exception as exc:
    preview = json.dumps(data, separators=(",", ":"))[:400]
    raise SystemExit(f"json_value({path_arg}) failed: {exc}; current={preview}")

if isinstance(data, (dict, list)):
    json.dump(data, sys.stdout, separators=(",", ":"))
elif isinstance(data, bool):
    sys.stdout.write("true" if data else "false")
elif data is None:
    pass
else:
    sys.stdout.write(str(data))
' "$path"
}

json_length() {
    python3 -c 'import json, sys; print(len(json.load(sys.stdin)))'
}

round_pixels() {
    python3 -c 'import sys; print(max(0, int(round(float(sys.argv[1])))))' "$1"
}

webdriver_request() {
    local method="$1"
    local path="$2"
    if [[ $# -ge 3 ]]; then
        local body="$3"
        curl --silent --show-error -X "$method" \
            -H 'Content-Type: application/json' \
            -d "$body" \
            "http://127.0.0.1:${PORT}${path}"
    else
        curl --silent --show-error -X "$method" \
            "http://127.0.0.1:${PORT}${path}"
    fi
}

webdriver_scroll() {
    local x="$1"
    local y="$2"
    local delta_y="$3"
    webdriver_request POST "/session/${SESSION_ID}/formal-web/scroll" \
        "{\"x\":${x},\"y\":${y},\"deltaY\":${delta_y}}" \
        >/dev/null
}

capture_screenshot() {
    local output_path="$1"
    local screenshot_response
    local screenshot_base64
    screenshot_response="$(webdriver_request GET "/session/${SESSION_ID}/screenshot")"
    screenshot_base64="$(printf '%s' "$screenshot_response" | json_value "value")"
    if [[ -z "$screenshot_base64" ]]; then
        fail_with_log "webdriver screenshot response omitted image data"
    fi
    python3 -c 'import base64, pathlib, sys; pathlib.Path(sys.argv[1]).write_bytes(base64.b64decode(sys.stdin.read().strip()))' \
        "$output_path" <<<"$screenshot_base64"
}

set_frame_region() {
    FRAME_X="$1"
    FRAME_Y="$2"
    FRAME_WIDTH="$3"
    FRAME_HEIGHT="$4"
    FRAME_CENTER_X=$(( FRAME_X + FRAME_WIDTH / 2 ))
    FRAME_CENTER_Y=$(( FRAME_Y + FRAME_HEIGHT / 2 ))
}

probe_iframe_region() {
    local probe_before="$WORK_DIR/probe-before.png"
    local probe_after="$WORK_DIR/probe-after.png"
    local candidate
    local x
    local y
    local width
    local height

    # First, attempt to scroll the startup artifact down using wheel input.
    for _ in {1..10}; do
        webdriver_scroll 680 560 220
    done

    # Candidate regions for the cross-origin iframe in the default headless viewport.
    for candidate in \
        "360 40 420 320" \
        "340 20 440 340" \
        "320 30 460 330" \
        "360 180 420 320" \
        "320 200 460 300"; do
        read -r x y width height <<<"$candidate"
        set_frame_region "$x" "$y" "$width" "$height"

        capture_screenshot "$probe_before"
        if ! "$SCREENSHOT_CHECKER" visible --png "$probe_before" --x "$FRAME_X" --y "$FRAME_Y" --width "$FRAME_WIDTH" --height "$FRAME_HEIGHT" >/dev/null 2>&1; then
            continue
        fi

        webdriver_scroll "$FRAME_CENTER_X" "$FRAME_CENTER_Y" 220
        sleep 0.15
        capture_screenshot "$probe_after"
        if "$SCREENSHOT_CHECKER" diff --before "$probe_before" --after "$probe_after" --x "$FRAME_X" --y "$FRAME_Y" --width "$FRAME_WIDTH" --height "$FRAME_HEIGHT" >/dev/null 2>&1; then
            return 0
        fi
    done

    return 1
}

cd "$ROOT"
rustup run 1.92.0 cargo build --release -p formal-web -p content -p net
rustup run 1.92.0 cargo build --release -p verification --bin webdriver-screenshot-check

if [[ ! -x "$SCREENSHOT_CHECKER" ]]; then
    echo "missing screenshot checker binary: $SCREENSHOT_CHECKER" >&2
    exit 1
fi

"$ROOT/target/release/formal-web" webdriver --headless --port "$PORT" --startup-url "$STARTUP_URL" \
    >"$LOG_FILE" 2>&1 &
FORMAL_WEB_PID="$!"

status_response=""
for _ in {1..200}; do
    if ! kill -0 "$FORMAL_WEB_PID" 2>/dev/null; then
        fail_with_log "formal-web exited before the webdriver server became ready"
    fi
    status_response="$(curl --silent --show-error "http://127.0.0.1:${PORT}/status" 2>/dev/null || true)"
    if [[ "$status_response" == *'"ready":true'* ]]; then
        break
    fi
    sleep 0.1
done

if [[ "$status_response" != *'"ready":true'* ]]; then
    fail_with_log "webdriver server did not become ready on port ${PORT}"
fi

session_response="$(webdriver_request POST "/session" '{}')"
SESSION_ID="$(printf '%s' "$session_response" | json_value "value.sessionId")"
if [[ -z "$SESSION_ID" ]]; then
    echo "failed to create webdriver session" >&2
    printf '%s\n' "$session_response" >&2
    exit 1
fi

if ! probe_iframe_region; then
    fail_with_log "failed to detect a visible, scroll-reactive cross-origin iframe region in startup screenshots"
fi

visible_screenshot="$WORK_DIR/iframe-visible.png"
before_scroll_screenshot="$WORK_DIR/iframe-before-scroll.png"
after_scroll_screenshot="$WORK_DIR/iframe-after-scroll.png"

capture_screenshot "$visible_screenshot"
visible_output="$($SCREENSHOT_CHECKER visible --png "$visible_screenshot" --x "$FRAME_X" --y "$FRAME_Y" --width "$FRAME_WIDTH" --height "$FRAME_HEIGHT" 2>&1)" || fail_with_log "cross-origin iframe content did not appear in screenshots: ${visible_output}"
printf '%s\n' "$visible_output"

capture_screenshot "$before_scroll_screenshot"
webdriver_scroll "$FRAME_CENTER_X" "$FRAME_CENTER_Y" 220
sleep 0.15
capture_screenshot "$after_scroll_screenshot"
diff_output="$($SCREENSHOT_CHECKER diff --before "$before_scroll_screenshot" --after "$after_scroll_screenshot" --x "$FRAME_X" --y "$FRAME_Y" --width "$FRAME_WIDTH" --height "$FRAME_HEIGHT" 2>&1)" || fail_with_log "cross-origin iframe did not show a scroll-driven visual change: ${diff_output}"
printf '%s\n' "$diff_output"

webdriver_request DELETE "/session/${SESSION_ID}" >/dev/null
SESSION_ID=""

echo "Rendering verification succeeded with screenshot-based iframe checks"