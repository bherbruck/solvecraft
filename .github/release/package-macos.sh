#!/usr/bin/env bash
# Package the macOS release build: a universal (arm64 + x86_64) SolveCraft.app in a .dmg (with an
# Applications link to drag it onto), and the universal CLI as a plain binary.
# Usage: .github/release/package-macos.sh <tag>   (from the repository root, after building both
# aarch64-apple-darwin and x86_64-apple-darwin). Writes out/. Neither is signed.
set -euo pipefail
tag=${1:?usage: package-macos.sh <tag>}
version=${tag#v}
t=${CARGO_TARGET_DIR:-target}
rm -rf stage out && mkdir -p stage/dmg out
app="stage/dmg/SolveCraft.app"
mkdir -p "$app/Contents/MacOS"
lipo -create -output "$app/Contents/MacOS/solvecraft" "$t/aarch64-apple-darwin/release/solvecraft" "$t/x86_64-apple-darwin/release/solvecraft"
lipo -create -output "out/solvecraft-cli-$tag-macos-universal" "$t/aarch64-apple-darwin/release/solvecraft-cli" "$t/x86_64-apple-darwin/release/solvecraft-cli"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>SolveCraft</string>
  <key>CFBundleDisplayName</key><string>SolveCraft</string>
  <key>CFBundleIdentifier</key><string>io.github.bherbruck.solvecraft</string>
  <key>CFBundleExecutable</key><string>solvecraft</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
lipo -info "$app/Contents/MacOS/solvecraft"
ln -s /Applications stage/dmg/Applications
hdiutil create -volname "SolveCraft $tag" -srcfolder stage/dmg -ov -format UDZO "out/SolveCraft-$tag-macos-universal.dmg"
# Check the image: mount it and look at what a user will see.
mnt=$(mktemp -d)
hdiutil attach -readonly -nobrowse -mountpoint "$mnt" "out/SolveCraft-$tag-macos-universal.dmg"
ls -la "$mnt" "$mnt/SolveCraft.app/Contents" "$mnt/SolveCraft.app/Contents/MacOS"
test -L "$mnt/Applications" && test "$(readlink "$mnt/Applications")" = /Applications
plutil -lint "$mnt/SolveCraft.app/Contents/Info.plist"
lipo "$mnt/SolveCraft.app/Contents/MacOS/solvecraft" -verify_arch arm64 x86_64
"$mnt/SolveCraft.app/Contents/MacOS/solvecraft" --version
hdiutil detach "$mnt"
ls -l out
