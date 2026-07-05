# hiroz WASM Demo — Multi-threaded

hiroz (ros-z) on the **multi-threaded** WASM runtime (`wasm-threads` feature of
the zenoh fork): SharedArrayBuffer Web Workers running pure-Rust executors,
talking to a ROS 2 Jazzy system via rmw_zenoh. Verified bidirectional:

- ROS 2 `talker` → rmw_zenoh → zenoh router → **browser subscriber** (threadpool worker)
- **Browser publisher** → zenoh router → rmw_zenoh → ROS 2 `listener`
  (`[listener]: I heard: [Hello from threaded WASM hiroz!]`)

## Architecture

```
Browser tab
  main thread  ── flume channels ──►  Application worker (LocalExecutor)
  (UI, polls)                           zenoh session + hiroz node
                                        sub + pub on /chatter
                                              │ (TX/RX/Net workers, Acceptor I/O worker)
                                              ▼ WebSocket :7448
                                        zenoh router (docker)
                                              ▼ TCP :7447
                                        ROS 2 Jazzy + rmw_zenoh_cpp (docker)
                                        demo_nodes_cpp talker / listener
```

The whole ROS stack lives in one task on the Application worker; the main
thread only moves strings through flume channels (`ros_poll`/`ros_publish`).
Nothing on the main thread ever blocks.

## Build & run

Requires nightly Rust (build-std; flags in `.cargo/config.toml`) and
`wasm-bindgen-cli` matching the crate version in Cargo.lock:

```sh
cargo install wasm-bindgen-cli --version 0.2.126
./build.sh

# ROS 2 stack (zenoh router + Jazzy talker/listener), reused from ../wasm-demo
docker compose -f ../wasm-demo/docker-compose.yml up -d

# SharedArrayBuffer needs cross-origin isolation → COOP/COEP server
python3 serve.py 8083

# Automated test (headless Chrome via puppeteer-core)
node run_headless.mjs

# Verify the browser→ROS direction in the listener's log:
docker compose -f ../wasm-demo/docker-compose.yml logs ros2 | grep "threaded WASM"

# Interactive page: open http://localhost:8083 — connect, watch the live
# /chatter feed from the ROS 2 talker, publish your own messages.
```

## Notes

- `node_modules` is a symlink into `zenoh-wasm/examples/wasm-threaded`
  (puppeteer-core only, used by the headless runner).
- If `ros_start` returns false, the page isn't cross-origin isolated —
  serve it through `serve.py`, not a plain HTTP server.
- The single-threaded variant with wasm-pack tests lives in `../wasm-demo`.
