#!/usr/bin/env bash
# Build the demo, bring up the ROS 2 docker stack, and run the headless
# end-to-end test (browser ↔ zenoh router ↔ ROS 2 Jazzy via rmw_zenoh).
#
# Usage: ./run-tests.sh [--keep-stack]
#   --keep-stack   leave the docker compose stack running afterwards
#
# Requirements: nightly Rust, wasm-bindgen-cli 0.2.126, docker compose,
#               python3, node + puppeteer-core, Chrome.
set -euo pipefail
cd "$(dirname "$0")"

KEEP_STACK="${1:-}"
SERVE_PID=""
cleanup() {
    [ -n "$SERVE_PID" ] && kill "$SERVE_PID" 2>/dev/null || true
    if [ "$KEEP_STACK" != "--keep-stack" ]; then
        docker compose down
    fi
}
trap cleanup EXIT

./build.sh

echo "=== Starting ROS 2 stack (zenoh router + Jazzy talker/listener) ==="
docker compose up -d --wait
echo "Waiting for ROS 2 nodes..."
sleep 8

python3 serve.py 8083 &
SERVE_PID=$!
for _ in $(seq 20); do
    python3 -c 'import socket; socket.create_connection(("127.0.0.1", 8083), 1)' 2>/dev/null && break
    sleep 0.5
done

node run_headless.mjs 60

echo ""
echo "=== browser → ROS 2 direction (listener log) ==="
docker compose logs ros2 2>&1 | grep "threaded WASM" | tail -3 \
    || { echo "FAIL: listener never heard the browser's message"; exit 1; }
echo "OK"
