#!/usr/bin/env bash
# Build the threaded hiroz demo (nightly + build-std; flags in .cargo/config.toml).
set -euo pipefail
cd "$(dirname "$0")"

cargo build --target wasm32-unknown-unknown --release

# CLI schema version must match the wasm-bindgen crate in Cargo.lock (0.2.126):
#   cargo install wasm-bindgen-cli --version 0.2.126
WASM_BINDGEN="${WASM_BINDGEN:-wasm-bindgen}"

"$WASM_BINDGEN" \
  --target no-modules \
  --out-dir pkg \
  target/wasm32-unknown-unknown/release/hiroz_wasm_demo.wasm

echo ""
echo "Build complete. Run:"
echo "  docker compose up -d                       # ROS 2 stack + router"
echo "  python3 serve.py 8083                      # COEP server"
echo "  node run_headless.mjs                      # automated test"
echo "  # or open http://localhost:8083 for the interactive page"
