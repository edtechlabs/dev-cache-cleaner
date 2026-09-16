#!/usr/bin/env bash
# install_app.sh — install macOS Dev Cache Cleaner to /Applications
#
# Usage:
#   ./install_app.sh              # finds built .app automatically, installs to /Applications
#   ./install_app.sh --build      # build first, then install
#   ./install_app.sh --dmg        # open the .dmg instead (manual drag-to-Applications)
#   ./install_app.sh --uninstall  # remove from /Applications
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_NAME="macOS Dev Cache Cleaner"
APP_BUNDLE="${APP_NAME}.app"
INSTALL_DIR="/Applications"
BUNDLE_DIR="$SCRIPT_DIR/src-tauri/target/release/bundle"
APP_SRC="$BUNDLE_DIR/macos/${APP_BUNDLE}"
DMG_SRC="$BUNDLE_DIR/dmg/${APP_NAME}_0.1.0_aarch64.dmg"

# ── helpers ───────────────────────────────────────────────────────────────────

die() { echo "error: $*" >&2; exit 1; }

find_dmg() {
  # pick most recent .dmg in the bundle dir
  find "$BUNDLE_DIR/dmg" -name "*.dmg" 2>/dev/null | sort -V | tail -1
}

# ── parse args ────────────────────────────────────────────────────────────────

BUILD_FIRST=false
OPEN_DMG=false
UNINSTALL=false

for arg in "$@"; do
  case $arg in
    --build)     BUILD_FIRST=true ;;
    --dmg)       OPEN_DMG=true ;;
    --uninstall) UNINSTALL=true ;;
    -h|--help)
      grep '^#' "$0" | head -8 | sed 's/^# \{0,2\}//'
      exit 0 ;;
    *) die "Unknown option: $arg" ;;
  esac
done

# ── uninstall ─────────────────────────────────────────────────────────────────

if [[ "$UNINSTALL" == "true" ]]; then
  TARGET="$INSTALL_DIR/$APP_BUNDLE"
  if [[ ! -e "$TARGET" ]]; then
    echo "${APP_BUNDLE} is not installed in ${INSTALL_DIR}."
    exit 0
  fi
  echo "Moving ${APP_BUNDLE} to Trash…"
  osascript -e "tell application \"Finder\" to delete POSIX file \"${TARGET}\"" >/dev/null
  echo "Uninstalled: ${TARGET}"
  exit 0
fi

# ── optional build ────────────────────────────────────────────────────────────

if [[ "$BUILD_FIRST" == "true" ]]; then
  echo "==> Building ${APP_BUNDLE}…"
  cd "$SCRIPT_DIR"
  [[ ! -d node_modules ]] && npm install
  npm run tauri:build
fi

# ── open DMG (manual install) ─────────────────────────────────────────────────

if [[ "$OPEN_DMG" == "true" ]]; then
  DMG=$(find_dmg)
  [[ -z "$DMG" ]] && die ".dmg not found. Run with --build first."
  echo "Opening: $DMG"
  open "$DMG"
  echo ""
  echo "Drag '${APP_BUNDLE}' into Applications to install."
  exit 0
fi

# ── direct install to /Applications ──────────────────────────────────────────

[[ -f "$APP_SRC/Contents/Info.plist" ]] || die \
  ".app not found at: $APP_SRC\nRun with --build to compile it first."

DEST="$INSTALL_DIR/$APP_BUNDLE"

# Remove existing version
if [[ -e "$DEST" ]]; then
  echo "Replacing existing installation…"
  rm -rf "$DEST"
fi

echo "Installing ${APP_BUNDLE} → ${INSTALL_DIR}…"
cp -R "$APP_SRC" "$DEST"

# Strip Gatekeeper quarantine so it opens without a security warning
xattr -cr "$DEST" 2>/dev/null || true

# Refresh Launch Services database
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
  -f "$DEST" 2>/dev/null || true

APP_SIZE=$(du -sh "$DEST" | cut -f1)
echo ""
echo "Installed: ${DEST} (${APP_SIZE})"
echo ""
echo "To launch:    open -a '${APP_NAME}'"
echo "To uninstall: $0 --uninstall"
