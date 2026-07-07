# hiroz WASM Demo

hiroz (ros-z) compiled to multi-threaded WebAssembly (`wasm-threads` feature of
the zenoh fork): SharedArrayBuffer Web Workers running pure-Rust executors,
talking to a ROS 2 Jazzy system via rmw_zenoh. Verified bidirectional:

- ROS 2 `talker` → rmw_zenoh → zenoh router → **browser subscriber** (threadpool worker)
- **Browser publisher** → zenoh router → rmw_zenoh → ROS 2 `listener`
  (`[listener]: I heard: [Hello from threaded WASM hiroz!]`)

Live instance: <https://0x53a.github.io/hiroz-web/hiroz/>

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

`std_msgs/String` comes from `hiroz-msgs` (generated at build time from the
vendored message definitions, including the RIHS01 type hash) — no hand-written
message types.

## Build & run

Requires nightly Rust (build-std; flags in `.cargo/config.toml`) and
`wasm-bindgen-cli` matching the crate version in Cargo.lock:

```sh
cargo install wasm-bindgen-cli --version 0.2.126

# everything below in one go: build, docker stack, headless e2e test
./run-tests.sh

# — or step by step —
./build.sh
docker compose up -d          # zenoh router + ROS 2 Jazzy talker/listener
python3 serve.py 8083         # COOP/COEP server (SharedArrayBuffer)
node run_headless.mjs         # automated test (headless Chrome)

# Verify the browser→ROS direction in the listener's log:
docker compose logs ros2 | grep "threaded WASM"

# Interactive page: open http://localhost:8083 — connect, watch the live
# /chatter feed from the ROS 2 talker, publish your own messages.
```

## Notes

- `node_modules` is a symlink into `zenoh-wasm/examples/wasm-threaded`
  (puppeteer-core only, used by the headless runner).
- If `ros_start` returns false, the page isn't cross-origin isolated —
  serve it through `serve.py`, not a plain HTTP server. On hosts that can't
  send COOP/COEP headers (e.g. GitHub Pages) the page falls back to the
  `coi-serviceworker.js` shim.
