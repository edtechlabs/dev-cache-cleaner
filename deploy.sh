#!/usr/bin/env bash
# deploy.sh — build the cache-cleaner CLI and produce a self-contained archive
#
# Usage:
#   ./deploy.sh              # release build for current arch
#   ./deploy.sh --debug      # debug build (faster, larger)
#   ./deploy.sh --universal  # fat binary: arm64 + x86_64 (requires both toolchains)
set -euo pipefail

# ── config ────────────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_DIR="$SCRIPT_DIR/src-tauri"
DIST_DIR="$SCRIPT_DIR/dist"

VERSION=$(grep '^version' "$CARGO_DIR/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')
HOST_ARCH=$(uname -m)  # arm64 or x86_64

BUILD_TYPE="release"
UNIVERSAL=false

for arg in "$@"; do
  case $arg in
    --debug)     BUILD_TYPE="debug" ;;
    --universal) UNIVERSAL=true ;;
    *) echo "Unknown arg: $arg"; exit 1 ;;
  esac
done

ARCH_LABEL="$HOST_ARCH"
if [[ "$UNIVERSAL" == "true" ]]; then ARCH_LABEL="universal"; fi

PKG_NAME="cache-cleaner-${VERSION}-macos-${ARCH_LABEL}"
PKG_DIR="$DIST_DIR/$PKG_NAME"
TARBALL="$DIST_DIR/${PKG_NAME}.tar.gz"

# ── build ─────────────────────────────────────────────────────────────────────

echo "==> cache-cleaner ${VERSION} (${ARCH_LABEL}, ${BUILD_TYPE})"

if [[ "$UNIVERSAL" == "true" ]]; then
  echo "==> Building arm64…"
  cargo build --manifest-path "$CARGO_DIR/Cargo.toml" \
    --release --bin cache-cleaner --target aarch64-apple-darwin

  echo "==> Building x86_64…"
  cargo build --manifest-path "$CARGO_DIR/Cargo.toml" \
    --release --bin cache-cleaner --target x86_64-apple-darwin

  BINARY_SRC=""  # handled separately below
else
  CARGO_FLAGS=(--manifest-path "$CARGO_DIR/Cargo.toml" --bin cache-cleaner)
  [[ "$BUILD_TYPE" == "release" ]] && CARGO_FLAGS+=(--release)
  echo "==> cargo build ${CARGO_FLAGS[*]}"
  cargo build "${CARGO_FLAGS[@]}"
  BINARY_SRC="$CARGO_DIR/target/${BUILD_TYPE}/cache-cleaner"
fi

# ── assemble package ──────────────────────────────────────────────────────────

echo "==> Assembling package…"
rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR"

if [[ "$UNIVERSAL" == "true" ]]; then
  lipo -create \
    "$CARGO_DIR/target/aarch64-apple-darwin/release/cache-cleaner" \
    "$CARGO_DIR/target/x86_64-apple-darwin/release/cache-cleaner" \
    -output "$PKG_DIR/cache-cleaner"
else
  cp "$BINARY_SRC" "$PKG_DIR/cache-cleaner"
fi

chmod +x "$PKG_DIR/cache-cleaner"

BINARY_SIZE=$(du -sh "$PKG_DIR/cache-cleaner" | cut -f1)
echo "    binary  : $PKG_DIR/cache-cleaner ($BINARY_SIZE)"

# ── generate install.sh ───────────────────────────────────────────────────────

cat > "$PKG_DIR/install.sh" << 'INSTALL_EOF'
#!/usr/bin/env bash
# install.sh — install cache-cleaner to a local bin directory
#
# Usage:
#   ./install.sh                    # installs to ~/.local/bin
#   ./install.sh --system           # installs to /usr/local/bin  (needs sudo)
#   ./install.sh --dir /my/bin      # custom install directory
#   ./install.sh --service          # also configure LaunchAgent after install
#   ./install.sh --service weekly   # weekly (default) | daily
#   ./install.sh --service-hour 4   # run at 4 AM instead of 3 AM
#   ./install.sh --allow-commands   # let service run brew/docker/etc commands too
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$HERE/cache-cleaner"

# ── parse args ─────────────────────────────────────────────────────────────────

INSTALL_DIR=""
USE_SYSTEM=false
SETUP_SERVICE=false
SERVICE_SCHEDULE="weekly"
SERVICE_HOUR=3
ALLOW_COMMANDS=false

while [[ $# -gt 0 ]]; do
  case $1 in
    --system)          USE_SYSTEM=true ;;
    --dir)             shift; INSTALL_DIR="$1" ;;
    --service)
      SETUP_SERVICE=true
      # accept optional positional: --service daily
      if [[ ${2:-} == "daily" || ${2:-} == "weekly" ]]; then
        SERVICE_SCHEDULE="$2"; shift
      fi
      ;;
    --service-hour)    shift; SERVICE_HOUR="$1" ;;
    --allow-commands)  ALLOW_COMMANDS=true ;;
    -h|--help)
      grep '^#' "$0" | sed 's/^# \{0,2\}//'
      exit 0 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
  shift
done

if [[ "$USE_SYSTEM" == "true" ]]; then
  INSTALL_DIR="/usr/local/bin"
elif [[ -z "$INSTALL_DIR" ]]; then
  INSTALL_DIR="$HOME/.local/bin"
fi

# ── install binary ─────────────────────────────────────────────────────────────

echo "Installing cache-cleaner → $INSTALL_DIR/cache-cleaner"
mkdir -p "$INSTALL_DIR"

if [[ "$USE_SYSTEM" == "true" ]]; then
  sudo install -m 755 "$BINARY" "$INSTALL_DIR/cache-cleaner"
else
  install -m 755 "$BINARY" "$INSTALL_DIR/cache-cleaner"
fi

# Remove quarantine flag set by macOS on downloaded binaries
xattr -d com.apple.quarantine "$INSTALL_DIR/cache-cleaner" 2>/dev/null || true

echo "Installed: $INSTALL_DIR/cache-cleaner"
echo "Version  : $("$INSTALL_DIR/cache-cleaner" --version 2>/dev/null || echo '?')"

# ── PATH check ────────────────────────────────────────────────────────────────

if ! echo ":$PATH:" | grep -q ":$INSTALL_DIR:"; then
  echo ""
  echo "Note: $INSTALL_DIR is not in your PATH."
  SHELL_RC="$HOME/.zshrc"
  [[ "$SHELL" == */bash ]] && SHELL_RC="$HOME/.bashrc"
  echo "Add this line to $SHELL_RC then open a new terminal:"
  echo ""
  echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
  echo ""
fi

# ── optional service setup ────────────────────────────────────────────────────

if [[ "$SETUP_SERVICE" == "true" ]]; then
  echo ""
  echo "Setting up automated cleaning service ($SERVICE_SCHEDULE at ${SERVICE_HOUR}:00)…"
  CMD=("$INSTALL_DIR/cache-cleaner" service install
    --schedule "$SERVICE_SCHEDULE"
    --hour "$SERVICE_HOUR"
    --bin "$INSTALL_DIR/cache-cleaner")
  [[ "$ALLOW_COMMANDS" == "true" ]] && CMD+=(--allow-commands)
  "${CMD[@]}"
else
  echo ""
  echo "To set up automated scheduled cleaning:"
  echo "  $INSTALL_DIR/cache-cleaner service install"
  echo ""
  echo "Quick start:"
  echo "  cache-cleaner scan"
  echo "  cache-cleaner dry-run --defaults"
  echo "  cache-cleaner clean --defaults --yes"
fi
INSTALL_EOF

# ── generate uninstall.sh ─────────────────────────────────────────────────────

cat > "$PKG_DIR/uninstall.sh" << 'UNINSTALL_EOF'
#!/usr/bin/env bash
# uninstall.sh — remove cache-cleaner and its LaunchAgent service
set -euo pipefail

SEARCH_DIRS=("$HOME/.local/bin" "/usr/local/bin" "/opt/homebrew/bin" "/usr/bin")

# ── stop service if running ───────────────────────────────────────────────────

PLIST="$HOME/Library/LaunchAgents/com.macos-dev-cache-cleaner.plist"
if [[ -f "$PLIST" ]]; then
  echo "Stopping LaunchAgent service…"
  launchctl unload -w "$PLIST" 2>/dev/null || true
  rm -f "$PLIST"
  echo "Removed: $PLIST"
fi

# ── remove binary ─────────────────────────────────────────────────────────────

FOUND=false
for DIR in "${SEARCH_DIRS[@]}"; do
  TARGET="$DIR/cache-cleaner"
  if [[ -f "$TARGET" ]]; then
    if [[ -w "$DIR" ]]; then
      rm -f "$TARGET"
    else
      sudo rm -f "$TARGET"
    fi
    echo "Removed: $TARGET"
    FOUND=true
  fi
done

if [[ "$FOUND" == "false" ]]; then
  echo "Binary not found in standard locations. Nothing removed."
fi

# ── optional: remove log ──────────────────────────────────────────────────────

LOG="$HOME/Library/Logs/dev-cache-cleaner-autorun.log"
if [[ -f "$LOG" ]]; then
  read -r -p "Remove service log $LOG? [y/N] " ans
  if [[ "$ans" =~ ^[Yy]$ ]]; then
    rm -f "$LOG"
    echo "Removed: $LOG"
  fi
fi

echo "Done."
UNINSTALL_EOF

chmod +x "$PKG_DIR/install.sh" "$PKG_DIR/uninstall.sh"
echo "    install  : $PKG_DIR/install.sh"
echo "    uninstall: $PKG_DIR/uninstall.sh"

# ── tarball ───────────────────────────────────────────────────────────────────

echo "==> Creating tarball…"
(cd "$DIST_DIR" && tar czf "${PKG_NAME}.tar.gz" "$PKG_NAME")
rm -rf "$PKG_DIR"

TARBALL_SIZE=$(du -sh "$TARBALL" | cut -f1)
echo ""
echo "==> Done."
echo "    $TARBALL ($TARBALL_SIZE)"
echo ""
echo "Deploy to a target machine:"
echo "  scp $TARBALL user@host:~/"
echo "  ssh user@host 'tar xzf ~/${PKG_NAME}.tar.gz && ./${PKG_NAME}/install.sh --service'"
