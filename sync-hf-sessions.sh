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
hf upload formal-web/pi-coding-sessions "$COLLECTED/" --repo-type=dataset --create-pr

echo ""
echo "=== Clearing local collected-sessions directory ==="
rm -rf "$COLLECTED"/*

echo "=== Done ==="
