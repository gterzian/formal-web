#!/usr/bin/env bash
set -euo pipefail

# This flow verifies hyperlink navigation and shutdown-time TLA validation.
# It intentionally does not perform screenshot-based rendering checks.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${FORMAL_WEB_WEBDRIVER_PORT:-4451}"
TLA2TOOLS_JAR="${FORMAL_WEB_TLA2TOOLS_JAR:-/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar}"
TLC_WORKERS="${FORMAL_WEB_TLC_WORKERS:-8}"
STARTUP_URL="file://${ROOT}/artifacts/StartupExample.html"
TARGET_URL="file://${ROOT}/artifacts/navigated.html"

if [[ -n "${FORMAL_WEB_VERIFY_WORK_DIR:-}" ]]; then
    WORK_DIR="$FORMAL_WEB_VERIFY_WORK_DIR"
    WORK_DIR_CREATED=0
    mkdir -p "$WORK_DIR"
else
    WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/formal-web-verify-navigation.XXXXXX")"
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

if [[ ! -f "$TLA2TOOLS_JAR" ]]; then
    echo "missing TLA+ Toolbox jar: $TLA2TOOLS_JAR" >&2
    exit 1
fi

cd "$ROOT"
rustup run 1.92.0 cargo build --release -p formal-web -p content -p net

FORMAL_WEB_TLA2TOOLS_JAR="$TLA2TOOLS_JAR" \
FORMAL_WEB_TLC_WORKERS="$TLC_WORKERS" \
"$ROOT/target/release/formal-web" --verify webdriver --headless --port "$PORT" --startup-url "$STARTUP_URL" \
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

webdriver_request POST "/session/${SESSION_ID}/formal-web/element/click" '{"selector":"a.article-link"}' >/dev/null

current_url=""
for _ in {1..200}; do
    if ! kill -0 "$FORMAL_WEB_PID" 2>/dev/null; then
        fail_with_log "formal-web exited before navigation reached the target artifact"
    fi
    url_response="$(webdriver_request GET "/session/${SESSION_ID}/url")"
    current_url="$(printf '%s' "$url_response" | json_value "value")"
    if [[ "$current_url" == "$TARGET_URL" ]]; then
        break
    fi
    sleep 0.1
done

if [[ "$current_url" != "$TARGET_URL" ]]; then
    fail_with_log "navigation did not reach ${TARGET_URL}; last URL: ${current_url}"
fi

webdriver_request DELETE "/session/${SESSION_ID}" >/dev/null
SESSION_ID=""

if ! wait "$FORMAL_WEB_PID"; then
    FORMAL_WEB_PID=""
    fail_with_log "formal-web exited with a failure status during verification"
fi
FORMAL_WEB_PID=""

if ! grep -Eq 'CHECK Navigation .* OK' "$LOG_FILE"; then
    fail_with_log "verification output did not report Navigation OK"
fi

echo "Navigation verification succeeded"