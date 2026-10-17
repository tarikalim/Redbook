#!/bin/sh
# Build release and install "/Applications/Redbook.app".
# scripts/bundle.sh <binary> <out dir> packages a prebuilt binary without installing.
# Icon: assets/icon.svg is the source; scripts/render-icon.swift renders it, and AppIcon.icns and
# redbook.png are made from that (iconutil, sips).
# The app's binary is the only one: Claude Code's MCP entry and the `redbook` CLI (a symlink in
# ~/.local/bin) run it too, so one bundle updates all three.
set -e
cd "$(dirname "$0")/.."
bin="${1:-target/release/redbook}"
app="${2:-/Applications}/Redbook.app"
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
[ -n "$1" ] || cargo build --release -j 4
# Drop the pre-rename bundle so only one app remains.
old="/Applications/Red Book.app"
if [ -z "$1" ] && [ -d "$old" ]; then
  "$lsregister" -u "$old" || true
  rm -rf "$old"
fi
# Updated in place, not deleted: deleting it dropped it from Spotlight until macOS indexed it again.
# The binary is moved over, not copied over, so a running `redbook mcp` keeps its old file.
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/redbook.new"
mv -f "$app/Contents/MacOS/redbook.new" "$app/Contents/MacOS/redbook"
cp assets/AppIcon.icns "$app/Contents/Resources/"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Redbook</string>
  <key>CFBundleDisplayName</key><string>Redbook</string>
  <key>CFBundleIdentifier</key><string>com.tarikalim.redbook</string>
  <key>CFBundleExecutable</key><string>redbook</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)</string>
  <key>CFBundleVersion</key><string>$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>LSEnvironment</key><dict><key>MallocSpaceEfficient</key><string>1</string></dict>
</dict></plist>
PLIST
codesign --force --deep -s - "$app"
# Updated in place, the bundle keeps its date and macOS keeps the cached icon; a new date refreshes it.
touch "$app"
[ -n "$1" ] || { "$lsregister" -f "$app"; mdimport "$app"; }
echo "installed: $app"
