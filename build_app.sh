#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

if [ ! -d node_modules ]; then
  npm install
fi

npm run tauri:build

echo "Built app bundles are under: src-tauri/target/release/bundle"
