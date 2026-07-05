# hiroz (ros-z) WASM Demo — Single-threaded

hiroz (formerly ros-z) compiled to `wasm32-unknown-unknown`, running in the browser, communicating bidirectionally with a ROS 2 Jazzy system via rmw_zenoh.

This is the **single-threaded** variant (stable toolchain, wasm-pack tests).
The **multi-threaded** variant (SharedArrayBuffer threadpool, interactive
page) lives in [`../wasm-demo-threaded/`](../wasm-demo-threaded/README.md)
and reuses this directory's docker compose stack.

## Architecture

```
Browser (WASM)          Zenoh Router          ROS 2 Jazzy
+-------------+        +------------+        +------------------+
| ros-z       |  WS    |            |  TCP   | rmw_zenoh_cpp    |
| ZContext    |------->|  zenohd    |<------>| demo_nodes_cpp   |
| ZNode       |  7448  |            |  7447  | talker / listener|
| ZPub / ZSub |        +------------+        +------------------+
+-------------+
```

## Running Tests

Requirements: `wasm-pack`, `geckodriver`, `firefox`, `docker compose`

```sh
# Basic tests (offline - CDR, key expressions, config)
nix-shell -p geckodriver --run "wasm-pack test --headless --firefox -- --test basic"

# Session tests (needs zenoh router on ws/127.0.0.1:7448)
docker run -d --rm --name zenoh-router -p 7448:7448 eclipse/zenoh:1.8.0 \
    --no-multicast-scouting --listen ws/0.0.0.0:7448
nix-shell -p geckodriver --run "wasm-pack test --headless --firefox -- --test session"
docker stop zenoh-router

# E2E tests (needs zenoh router + ROS 2 Jazzy with rmw_zenoh)
docker compose up -d
sleep 10  # wait for ROS 2 nodes to start
nix-shell -p geckodriver --run "wasm-pack test --headless --firefox -- --test e2e"
docker compose down

# Or use the helper script
./run-tests.sh basic
./run-tests.sh session
./run-tests.sh e2e
```

## Test Summary

| Test | What it proves |
|------|----------------|
| `cdr_roundtrip` | CDR serialization/deserialization works in WASM |
| `cdr_empty_string` | Edge case: empty string CDR roundtrip |
| `message_type_info` | Correct DDS type name for std_msgs/msg/String |
| `zenoh_config_for_ws` | WebSocket endpoint config creation |
| `rmw_zenoh_key_expression` | rmw_zenoh key expression format generation |
| `context_builder_creation` | ZContextBuilder instantiation |
| `rosz_session_open` | Open zenoh session + create ros-z node over WebSocket |
| `rosz_pubsub_roundtrip` | Full ros-z pub/sub stack through zenoh router |
| `subscribe_to_ros2_chatter` | Receive messages from ROS 2 Jazzy talker |
| `publish_to_ros2_chatter` | Send messages to ROS 2 Jazzy listener |

## Key Files

- `src/messages.rs` - Manual `std_msgs/msg/String` definition with Jazzy RIHS01 type hash
- `src/lib.rs` - WASM entry point
- `tests/basic.rs` - Offline tests
- `tests/session.rs` - Session tests (zenoh router required)
- `tests/e2e.rs` - End-to-end tests (ROS 2 Jazzy + rmw_zenoh required)
- `docker-compose.yml` - zenoh router + ROS 2 Jazzy stack
- `Dockerfile.ros2` - ROS 2 Jazzy + rmw_zenoh built from source

## Changes to ros-z for WASM

All changes are in `crates/hiroz/` (formerly `ros-z`):

- `src/compat.rs` (new) - parking_lot API wrapper over std::sync for WASM
- `Cargo.toml` - Platform-split deps (parking_lot, tokio, zenoh features, uuid/js)
- `src/lib.rs` - Gate `shm` module on non-WASM
- `src/context.rs` - Added `build_async()` for WASM session creation
- `src/time.rs` - `js_sys::Date::now()` instead of `SystemTime::now()` on WASM
- `src/graph.rs` - Skip blocking liveliness query on WASM
- `src/{pubsub,service,node,msg,zbuf,cache,queue,action/*}.rs` - cfg gates for shm, parking_lot, tokio_util

## Changes to zenoh-wasm

- `zenoh-ext/Cargo.toml` - Gate tokio `io-std` behind non-WASM
- `zenoh/src/lib.rs` - Gate `zenoh_home`, `LibLoader`, `Timer` exports
- `io/zenoh-transport/src/unicast/universal/tx.rs` - Gate `BlockFirst` congestion control
