#!/bin/sh
# Builds a macOS .app bundle so the menu bar and Dock show "Button Badger".
# Usage: packaging/bundle.sh [binary]   (default: builds target/release/button_badger)
set -e
cd "$(dirname "$0")/.."
BIN="$1"
if [ -z "$BIN" ]; then
  cargo build --release
  BIN=target/release/button_badger
fi
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
cargo run --release --quiet --example render_icon -- packaging/icon.svg target/AppIcon.iconset
iconutil -c icns target/AppIcon.iconset -o target/AppIcon.icns
APP="target/release/Button Badger.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp packaging/Info.plist "$APP/Contents/"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" -c "Set :CFBundleVersion $VERSION" "$APP/Contents/Info.plist"
cp target/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp "$BIN" "$APP/Contents/MacOS/Button Badger"
# Ad-hoc signature: not notarised, but Apple Silicon refuses to run unsigned bundles at all.
codesign --force --deep --sign - "$APP"
echo "Built $APP ($VERSION)"
