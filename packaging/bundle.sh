#!/bin/sh
# Builds a macOS .app bundle so the menu bar and Dock show "Button Badger".
set -e
cd "$(dirname "$0")/.."
cargo build --release
cargo run --release --quiet --example render_icon -- packaging/icon.svg target/AppIcon.iconset
iconutil -c icns target/AppIcon.iconset -o target/AppIcon.icns
APP="target/release/Button Badger.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp packaging/Info.plist "$APP/Contents/"
cp target/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp target/release/button_badger "$APP/Contents/MacOS/Button Badger"
echo "Built $APP"
