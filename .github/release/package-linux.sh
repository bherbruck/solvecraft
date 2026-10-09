#!/usr/bin/env bash
# Package the Linux release build: a tar.gz with both binaries, and an AppImage of the desktop app.
# Usage: .github/release/package-linux.sh <tag>   (from the repository root, after
# `cargo build --release -p solvecraft -p solvecraft-cli`). Writes out/.
set -euo pipefail
tag=${1:?usage: package-linux.sh <tag>}
bin=${CARGO_TARGET_DIR:-target}/release
name="solvecraft-$tag-linux-x86_64"
rm -rf stage out && mkdir -p "stage/$name" out
cp "$bin/solvecraft" "$bin/solvecraft-cli" README.md LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md "stage/$name/"
tar -C stage -czf "out/$name.tar.gz" "$name"

# AppImage. The icon is rendered by SolveCraft itself from the bracket example, so no image file
# is committed.
app=stage/SolveCraft.AppDir
mkdir -p "$app/usr/bin"
cp "$bin/solvecraft" "$bin/solvecraft-cli" "$app/usr/bin/"
"$bin/solvecraft-cli" snapshot examples/bracket.json --out "$app/solvecraft.png" --width 256 --height 256 >/dev/null
cat > "$app/solvecraft.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=SolveCraft
Comment=Parametric 3D CAD
Exec=solvecraft %f
Icon=solvecraft
Categories=Graphics;Engineering;
MimeType=model/step;
DESKTOP
cat > "$app/AppRun" <<'APPRUN'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/solvecraft" "$@"
APPRUN
chmod +x "$app/AppRun"
tool=stage/appimagetool
curl -sSfL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x "$tool"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" --no-appstream "$app" "out/SolveCraft-$tag-x86_64.AppImage"
ls -l out
