#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "$(uname -s)" == Linux ]] || { echo "Run this build on Linux." >&2; exit 1; }
npm run build
# Without this feature, even a release binary opens devUrl (localhost:1420).
cargo build --release --manifest-path src-tauri/Cargo.toml --features tauri/custom-protocol --bin codex-switcher
