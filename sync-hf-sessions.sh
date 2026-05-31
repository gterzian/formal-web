#!/usr/bin/env bash
set -euo pipefail

# sync-hf-sessions.sh
#
# Upload locally collected pi coding sessions to the Hugging Face dataset
# at https://huggingface.co/datasets/formal-web/pi-coding-sessions, then
# clear the local collected-sessions directory.
#
# Prerequisites:
#   - The `hf` CLI must be installed and authenticated
#     (see https://huggingface.co/docs/huggingface_hub/en/guides/cli)
#   - Write access to the formal-web/pi-coding-sessions dataset

COLLECTED=".pi/collected-sessions"

echo "=== Uploading collected sessions to HF dataset ==="

echo "Using hf upload to upload $COLLECTED/ to formal-web/pi-coding-sessions..."
if hf upload formal-web/pi-coding-sessions "$COLLECTED/" --repo-type=dataset --create-pr; then
    echo ""
    echo "=== Upload succeeded — clearing local collected-sessions directory ==="
    rm -rf "$COLLECTED"/*
    echo "=== Done ==="
else
    exit_code=$?
    echo ""
    echo "=== Upload FAILED (exit code $exit_code) — local collected sessions preserved in $COLLECTED/ ===" >&2
    exit "$exit_code"
fi
