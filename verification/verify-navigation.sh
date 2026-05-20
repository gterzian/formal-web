#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${FORMAL_WEB_WEBDRIVER_PORT:-4451}"
TLA2TOOLS_JAR="${FORMAL_WEB_TLA2TOOLS_JAR:-/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar}"
TLC_WORKERS="${FORMAL_WEB_TLC_WORKERS:-8}"
STARTUP_URL="file://${ROOT}/artifacts/StartupExample.html"
TARGET_URL="file://${ROOT}/artifacts/navigated.html"
LOG_FILE="${FORMAL_WEB_VERIFY_LOG_FILE:-$(mktemp "${TMPDIR:-/tmp}/formal-web-verify-navigation.XXXXXX")}"
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
        rm -f "$LOG_FILE"
    else
        echo "verification log: $LOG_FILE" >&2
    fi
}
trap cleanup EXIT

if [[ ! -f "$TLA2TOOLS_JAR" ]]; then
    echo "missing TLA+ Toolbox jar: $TLA2TOOLS_JAR" >&2
    exit 1
fi

cd "$ROOT"
rustup run 1.92.0 cargo build --release -p formal-web -p content -p net

# formal-web verification launches TLC with the local Toolbox jar, for example:
# java -jar "$TLA2TOOLS_JAR" -config verification/tla_specs/Navigation.cfg verification/tla_specs/Navigation.tla -workers "$TLC_WORKERS"
FORMAL_WEB_TLA2TOOLS_JAR="$TLA2TOOLS_JAR" \
FORMAL_WEB_TLC_WORKERS="$TLC_WORKERS" \
"$ROOT/target/release/formal-web" --verify webdriver --headless --port "$PORT" --startup-url "$STARTUP_URL" \
    >"$LOG_FILE" 2>&1 &
FORMAL_WEB_PID="$!"

status_response=""
for _ in {1..200}; do
    if ! kill -0 "$FORMAL_WEB_PID" 2>/dev/null; then
        tail -n 50 "$LOG_FILE" >&2
        exit 1
    fi
    status_response="$(curl --silent --show-error "http://127.0.0.1:${PORT}/status" 2>/dev/null || true)"
    if [[ "$status_response" == *'"ready":true'* ]]; then
        break
    fi
    sleep 0.1
done

if [[ "$status_response" != *'"ready":true'* ]]; then
    echo "webdriver server did not become ready on port ${PORT}" >&2
    tail -n 50 "$LOG_FILE" >&2
    exit 1
fi

session_response="$(curl --silent --show-error -X POST -H 'Content-Type: application/json' -d '{}' "http://127.0.0.1:${PORT}/session")"
SESSION_ID="$(printf '%s' "$session_response" | tr -d '\n' | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')"
if [[ -z "$SESSION_ID" ]]; then
    echo "failed to create webdriver session" >&2
    printf '%s\n' "$session_response" >&2
    exit 1
fi

curl --silent --show-error -X POST \
    -H 'Content-Type: application/json' \
    -d '{"selector":"a.article-link"}' \
    "http://127.0.0.1:${PORT}/session/${SESSION_ID}/formal-web/element/click" \
    >/dev/null

current_url=""
for _ in {1..200}; do
    if ! kill -0 "$FORMAL_WEB_PID" 2>/dev/null; then
        tail -n 50 "$LOG_FILE" >&2
        exit 1
    fi
    url_response="$(curl --silent --show-error "http://127.0.0.1:${PORT}/session/${SESSION_ID}/url")"
    current_url="$(printf '%s' "$url_response" | tr -d '\n' | sed -n 's/.*"value":"\([^"]*\)".*/\1/p')"
    if [[ "$current_url" == "$TARGET_URL" ]]; then
        break
    fi
    sleep 0.1
done

if [[ "$current_url" != "$TARGET_URL" ]]; then
    echo "navigation did not reach ${TARGET_URL}" >&2
    echo "last URL: ${current_url}" >&2
    tail -n 50 "$LOG_FILE" >&2
    exit 1
fi

curl --silent --show-error -X DELETE "http://127.0.0.1:${PORT}/session/${SESSION_ID}" >/dev/null
SESSION_ID=""

if ! wait "$FORMAL_WEB_PID"; then
    FORMAL_WEB_PID=""
    tail -n 50 "$LOG_FILE" >&2
    exit 1
fi
FORMAL_WEB_PID=""

if ! grep -Eq 'CHECK Navigation .* OK' "$LOG_FILE"; then
    echo "verification output did not report Navigation OK" >&2
    tail -n 50 "$LOG_FILE" >&2
    exit 1
fi

echo "Navigation verification succeeded"