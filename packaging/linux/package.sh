#!/usr/bin/env bash
# Build and package SolveCraft for Linux (<arch> is x86_64 or aarch64):
#
#   $DIST/solvecraft-<version>-linux-<arch>.AppImage        any distro with glibc >= the build host's
#   $DIST/solvecraft-<version>-linux-<arch>.AppImage.zsync  delta updates (needs zsyncmake)
#   $DIST/solvecraft-<version>-linux-<arch>.deb             Debian, Ubuntu, Mint, Pop!_OS, ...
#   $DIST/solvecraft-<version>-linux-<arch>.rpm             Fedora, openSUSE, RHEL, ...
#   $DIST/solvecraft-<version>-linux-<arch>.tar.gz          plain FHS-style tree (bin/, share/)
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm tar"]
#
# Needs: cargo; nfpm for deb/rpm (https://nfpm.goreleaser.com); appimagetool for the AppImage
# (downloaded into $CARGO_TARGET_DIR if missing). Build on an old distro (CI: Ubuntu 22.04,
# glibc 2.35) so the binaries run on newer ones. Optional: desktop-file-validate, appstreamcli,
# zsyncmake (the zsync package) for the AppImage's .zsync.
# Adapted from PhotoCraft's packaging (storytold/photocraft, Apache-2.0; see ATTRIBUTION.md).
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"

SKIP_BUILD=0
FORMATS="appimage deb rpm tar"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    -h | --help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
export SOLVECRAFT_MAINTAINER="${SOLVECRAFT_MAINTAINER:-SolveCraft contributors <https://github.com/bherbruck/solvecraft>}"
BASENAME="solvecraft-$VERSION-linux-$ARCH"

echo "==> SolveCraft $VERSION for Linux $ARCH ($FORMATS)"

if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p solvecraft -p solvecraft-cli)
fi
BIN="$CARGO_TARGET_DIR/release"
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- stage an FHS tree (shared by every format) -------------------------------------------------
install -Dm755 "$BIN/solvecraft" "$STAGE/usr/bin/solvecraft"
install -Dm755 "$BIN/solvecraft-cli" "$STAGE/usr/bin/solvecraft-cli"
strip "$STAGE/usr/bin/solvecraft" "$STAGE/usr/bin/solvecraft-cli" 2>/dev/null || true
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
install -Dm644 "$HERE/$APP_ID.mime.xml" "$STAGE/usr/share/mime/packages/$APP_ID.xml"
mkdir -p "$STAGE/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$SOLVECRAFT_BUILD_DATE/g" \
  "$HERE/$APP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
make_icons "$STAGE/usr/bin/solvecraft-cli" "$WORK/icons"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$WORK/icons/hicolor" "$STAGE/usr/share/icons/"
mkdir -p "$STAGE/usr/share/doc/solvecraft"
copy_docs "$STAGE/usr/share/doc/solvecraft"

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
fi

has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# ---- .tar.gz ------------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# ---- .deb / .rpm --------------------------------------------------------------------------------
if has deb || has rpm; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export VERSION
  export NFPM_ARCH="$DEB_ARCH"
  # nfpm expands env vars in fields like `version` and `arch`, but not in `contents[].src`.
  sed "s|\${STAGE}|$STAGE|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  for fmt in deb rpm; do
    if has "$fmt"; then (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt"); fi
  done
fi

# ---- AppImage -----------------------------------------------------------------------------------
if has appimage; then
  APPDIR="$WORK/SolveCraft.AppDir"
  cp -R "$STAGE" "$APPDIR"
  mv "$APPDIR/usr/share/doc" "$WORK/doc-unused"
  # AppRun is a script, not a symlink: it installs the .desktop entry and icons into
  # $XDG_DATA_HOME before launching, so Wayland compositors can resolve the window's app_id to
  # our icon.
  install -Dm755 "$HERE/AppRun" "$APPDIR/AppRun"
  cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
  cp "$WORK/icons/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"
  ln -s "$APP_ID.png" "$APPDIR/.DirIcon"

  TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
  if [ -z "$TOOL" ]; then
    TOOL="$CARGO_TARGET_DIR/appimagetool-$ARCH.AppImage"
    if [ ! -x "$TOOL" ]; then
      curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
      chmod +x "$TOOL"
    fi
  fi
  OUT="$DIST/$BASENAME.AppImage"
  APPDIR="$(cd "$APPDIR" && pwd)"
  TOOL="$(cd "$(dirname "$TOOL")" && pwd)/$(basename "$TOOL")"
  rm -f "$OUT.zsync"
  # Update information: AppImageUpdate and the like fetch only the changed blocks of a newer
  # release through the .zsync published next to each AppImage. `latest` is the newest published
  # release that is not a pre-release.
  REPO="${GITHUB_REPOSITORY:-bherbruck/solvecraft}"
  UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest|solvecraft-*-linux-$ARCH.AppImage.zsync"
  # Extract-and-run: works without FUSE (containers, CI). With zsyncmake on the host appimagetool
  # also writes the .zsync, into its working directory, hence the cd.
  (cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream -u "$UPDATE_INFO" "$APPDIR" "$OUT")
  echo "wrote $OUT"
  if [ -s "$OUT.zsync" ]; then
    echo "wrote $OUT.zsync"
  else
    warn "zsyncmake not found, so $OUT.zsync was not written; AppImage delta updates need it"
  fi
fi

"$STAGE/usr/bin/solvecraft-cli" --version
echo "==> done"
ls -lh "$DIST"
