#!/bin/bash
# Install XPC service plists for formal-web helper processes.
# This script substitutes __TARGET_DIR__ with the actual target directory
# and loads the plists into launchd.
#
# Usage:
#   ./xpc-services/install.sh [target_dir]
#
# If target_dir is not specified, it defaults to:
#   ./target/release

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TARGET_DIR="${1:-$REPO_ROOT/target/release}"

if [ ! -d "$TARGET_DIR" ]; then
    echo "Error: target directory '$TARGET_DIR' does not exist"
    echo "Build the project first with: cargo build --release"
    echo "Or specify a different target directory."
    exit 1
fi

for plist_source in "$SCRIPT_DIR"/*.plist; do
    basename=$(basename "$plist_source")
    label="${basename%.plist}"
    target_plist="$HOME/Library/LaunchAgents/$basename"

    # Substitute the target directory
    sed "s|__TARGET_DIR__|$TARGET_DIR|g" "$plist_source" > "$target_plist"

    echo "Installed $target_plist"
    echo "    (binary path: $TARGET_DIR/$label)"
done

echo ""
echo "To load the services, run:"
echo "  launchctl load ~/Library/LaunchAgents/formal-web.net.plist"
echo "  launchctl load ~/Library/LaunchAgents/formal-web.media.plist"
echo "  launchctl load ~/Library/LaunchAgents/formal-web.content.plist"
echo ""
echo "To unload:"
echo "  launchctl unload ~/Library/LaunchAgents/formal-web.net.plist"
echo "  launchctl unload ~/Library/LaunchAgents/formal-web.media.plist"
echo "  launchctl unload ~/Library/LaunchAgents/formal-web.content.plist"
