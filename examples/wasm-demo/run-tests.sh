#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

usage() {
    echo "Usage: $0 [basic|session|e2e|all]"
    echo ""
    echo "  basic   - Run offline tests (CDR, key expressions, config)"
    echo "  session - Run session tests against local zenoh router (starts/stops router)"
    echo "  e2e     - Run end-to-end tests against ROS 2 Jazzy via Docker"
    echo "  all     - Run all tests"
    echo ""
    echo "Requirements:"
    echo "  basic:   wasm-pack, geckodriver, firefox"
    echo "  session: wasm-pack, geckodriver, firefox, zenohd (or docker)"
    echo "  e2e:     wasm-pack, geckodriver, firefox, docker compose"
    exit 1
}

TEST_TYPE="${1:-all}"

ZENOHD_PID=""
cleanup() {
    if [ -n "$ZENOHD_PID" ]; then
        echo "Stopping zenoh router (PID $ZENOHD_PID)..."
        kill "$ZENOHD_PID" 2>/dev/null || true
        wait "$ZENOHD_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

start_router() {
    echo "Starting zenoh router on ws/127.0.0.1:7448..."

    # Try local zenohd first, fall back to Docker
    if command -v zenohd &>/dev/null; then
        zenohd --no-multicast-scouting -l ws/0.0.0.0:7448 &
        ZENOHD_PID=$!
    elif command -v docker &>/dev/null; then
        docker run -d --rm --name zenoh-test-router \
            -p 7447:7447 -p 7448:7448 \
            eclipse/zenoh:1.8.0 \
            --no-multicast-scouting --listen tcp/0.0.0.0:7447 --listen ws/0.0.0.0:7448
        ZENOHD_PID="docker"
    else
        echo "ERROR: Neither zenohd nor docker found. Cannot start router."
        exit 1
    fi

    # Wait for router to be ready
    echo "Waiting for router..."
    sleep 2
}

stop_router() {
    if [ "$ZENOHD_PID" = "docker" ]; then
        echo "Stopping Docker zenoh router..."
        docker stop zenoh-test-router 2>/dev/null || true
        ZENOHD_PID=""
    elif [ -n "$ZENOHD_PID" ]; then
        echo "Stopping zenoh router..."
        kill "$ZENOHD_PID" 2>/dev/null || true
        wait "$ZENOHD_PID" 2>/dev/null || true
        ZENOHD_PID=""
    fi
}

run_basic() {
    echo "=== Running basic (offline) tests ==="
    wasm-pack test --headless --firefox -- --test basic
    echo "=== Basic tests PASSED ==="
}

run_session() {
    echo "=== Running session tests (ros-z over WebSocket) ==="
    start_router
    wasm-pack test --headless --firefox -- --test session
    stop_router
    echo "=== Session tests PASSED ==="
}

run_e2e() {
    echo "=== Running end-to-end tests (ros-z WASM <-> ROS 2 Jazzy) ==="
    echo "Building ROS 2 Docker image (this may take a while on first run)..."
    docker compose up -d
    echo "Waiting for ROS 2 nodes to start..."
    sleep 10

    wasm-pack test --headless --firefox -- --test e2e

    docker compose logs --tail=20
    docker compose down
    echo "=== E2E tests PASSED ==="
}

case "$TEST_TYPE" in
    basic)
        run_basic
        ;;
    session)
        run_session
        ;;
    e2e)
        run_e2e
        ;;
    all)
        run_basic
        run_session
        ;;
    *)
        usage
        ;;
esac

echo ""
echo "All requested tests completed successfully."
