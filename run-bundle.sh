#!/bin/bash
# Build and run formal-web as a macOS .app bundle with embedded XPC services.
# This enables native XPC for the content process (bypasses launchd watchdog).
set -euo pipefail

cd "$(dirname "$0")"

APP_NAME="FormalWeb"
TARGET_DIR="target/release"
APP_BUNDLE="$TARGET_DIR/$APP_NAME.app"
XPC_BUNDLE="$APP_BUNDLE/Contents/XPCServices/com.formal-web.app.content.xpc"

echo "🦀 Building (content defaults to ipc-channel)..."
cargo build --release

echo "📦 Assembling macOS bundle..."
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$XPC_BUNDLE/Contents/MacOS"

cp "$TARGET_DIR/formal-web" "$APP_BUNDLE/Contents/MacOS/$APP_NAME"
cp "$TARGET_DIR/formal-web-content" "$XPC_BUNDLE/Contents/MacOS/content"

# Symlink helpers so the sidecar lookup finds them
ln -sf /Users/Gregory/Projects/formal-web/$TARGET_DIR/formal-web-net \
    "$APP_BUNDLE/Contents/MacOS/formal-web-net"
ln -sf /Users/Gregory/Projects/formal-web/$TARGET_DIR/formal-web-media \
    "$APP_BUNDLE/Contents/MacOS/formal-web-media"
ln -sf ../../XPCServices/com.formal-web.app.content.xpc/Contents/MacOS/content \
    "$APP_BUNDLE/Contents/MacOS/formal-web-content"

cat > "$APP_BUNDLE/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.formal-web.app</string>
    <key>CFBundleExecutable</key>
    <string>FormalWeb</string>
</dict>
</plist>
EOF

cat > "$XPC_BUNDLE/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.formal-web.app.content</string>
    <key>CFBundlePackageType</key>
    <string>XPC!</string>
    <key>CFBundleExecutable</key>
    <string>content</string>
    <key>XPCService</key>
    <dict>
        <key>ServiceType</key>
        <string>Application</string>
        <key>MultipleInstances</key>
        <true/>
    </dict>
</dict>
</plist>
EOF

echo "🚀 Launching from bundle..."
exec "$APP_BUNDLE/Contents/MacOS/$APP_NAME" "$@"
