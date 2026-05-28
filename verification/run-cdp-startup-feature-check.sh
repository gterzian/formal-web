#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.92.0}"
ARGS=("$@")

cleanup_formal_web_processes() {
  pkill -f 'formal-web-embedder cdp' >/dev/null 2>&1 || true
  pkill -f 'formal-web-content:' >/dev/null 2>&1 || true
  pkill -f 'formal-web-net ' >/dev/null 2>&1 || true
  pkill -f 'cdp-smoke-check' >/dev/null 2>&1 || true
}

trap cleanup_formal_web_processes EXIT

echo "[cdp-check] running startup artifact feature checks via external CDP client"
echo "[cdp-check] toolchain: ${RUSTUP_TOOLCHAIN}"

if [[ ${#ARGS[@]} -eq 0 ]]; then
  rustup run "${RUSTUP_TOOLCHAIN}" cargo run \
    --manifest-path verification/Cargo.toml \
    --bin cdp-smoke-check \
    -- --rebuild-browser
else
  rustup run "${RUSTUP_TOOLCHAIN}" cargo run \
    --manifest-path verification/Cargo.toml \
    --bin cdp-smoke-check \
    -- --rebuild-browser "${ARGS[@]}"
fi
