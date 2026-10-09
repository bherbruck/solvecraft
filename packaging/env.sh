# shellcheck shell=bash
# Shared setup for the packaging scripts. Source it: `. "$(dirname "$0")/../env.sh"`.
# Adapted from PhotoCraft's packaging (storytold/photocraft, Apache-2.0; see ATTRIBUTION.md).
#
# Exports:
#   ROOT                    workspace root
#   VERSION                 [workspace.package] version from Cargo.toml (override: SOLVECRAFT_VERSION)
#   DIST                    output directory for release artifacts (default: $ROOT/dist/release)
#   SOLVECRAFT_BUILD_DATE   UTC build date, YYYY-MM-DD
#   CARGO_TARGET_DIR        cargo's target dir (default: $ROOT/target)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

# The version lives in one place: `[workspace.package] version` in the root Cargo.toml.
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") ; next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
}

VERSION="${SOLVECRAFT_VERSION:-$(workspace_version)}"
if [ -z "$VERSION" ]; then
  echo "error: could not read [workspace.package] version from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist/release}"
mkdir -p "$DIST"
DIST="$(cd "$DIST" && pwd)"
export DIST

export SOLVECRAFT_BUILD_DATE="${SOLVECRAFT_BUILD_DATE:-$(date -u +%Y-%m-%d)}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
APP_ID=ai.storyteller.solvecraft
export APP_ID

# Emit a GitHub Actions warning (plain stderr outside Actions).
warn() {
  if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
}

# Copy the licence, notice and readme files into a package directory. The programs also carry
# them (Help > About > Licences, `solvecraft-cli licences`).
copy_docs() {
  local dest="$1" f
  for f in README.md LICENSE-MIT LICENSE-APACHE NOTICE THIRD-PARTY-LICENSES.txt; do
    if [ -f "$ROOT/$f" ]; then cp "$ROOT/$f" "$dest/"; fi
  done
}

# The app icon in every size a package needs, drawn by the CLI (no image files are committed):
# $1 = the solvecraft-cli to run, $2 = output dir. Writes hicolor/<n>x<n>/apps/$APP_ID.png,
# solvecraft.ico and solvecraft-1024.png.
make_icons() {
  local cli="$1" out="$2" s
  for s in 16 24 32 48 64 128 256 512; do
    mkdir -p "$out/hicolor/${s}x${s}/apps"
    "$cli" icon --size "$s" --out "$out/hicolor/${s}x${s}/apps/$APP_ID.png" >/dev/null
  done
  "$cli" icon --out "$out/solvecraft.ico" >/dev/null
  "$cli" icon --size 1024 --out "$out/solvecraft-1024.png" >/dev/null
}
