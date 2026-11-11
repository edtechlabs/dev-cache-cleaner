#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust/Cargo is missing. Install Rust first: https://www.rust-lang.org/tools/install" >&2
  exit 1
fi
if ! command -v node >/dev/null 2>&1; then
  echo "Node.js is missing. Install with Homebrew: brew install node" >&2
  exit 1
fi
if ! command -v npm >/dev/null 2>&1; then
  echo "npm is missing. Install Node.js first." >&2
  exit 1
fi

if [ ! -d node_modules ]; then
  npm install
fi

npm run tauri:dev
