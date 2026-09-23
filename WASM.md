# Browser WASM branch

This branch demonstrates Rust hiroz ROS nodes in a browser using the Zenoh WASM
fork. Upstream main was merged through
`c50384343167e43bc86f0b18681e670708449184`.

Use the [integration repository](https://github.com/0x53A/hiroz-web), cloned
recursively, for a reproducible checkout. This workspace's Cargo patches expect
the matching Zenoh fork at `../zenoh-wasm`; a standalone clone of this branch
needs that sibling checkout. The browser examples carry their own Cargo patches
because dependency-workspace patches do not propagate to consumers.

## WASM implementation

- Target-specific dependencies and `compat`: synchronization, clocks, spawning
  and task ownership without a Tokio execution runtime in browsers.
- Context `build_async` / `shutdown_async`, browser wall clocks, and discovery,
  service and queue deadlines.

## General hiroz corrections

These fixes also apply to native code and are separate from the browser runtime
adaptation.

- Action lifecycle corrections: cancellation selectors, request metadata checks,
  result retention/expiration, goal-instance identity, registration cleanup and
  an explicit `GoalRejected` error.
- Code generation: complete CancelGoal descriptions and correct action/service
  hash namespaces. Native regressions are beside the changes.

## Validation

Browser examples and ROS fixtures are in `examples/wasm-demo` in the integration
repository. They cover pub/sub, services, actions in both ROS/browser directions,
and a simulated device plus groundstation. Native checks in this workspace:

```sh
cargo test --locked -p hiroz --test action -- --test-threads=1
cargo test --locked -p hiroz --lib queue::timeout_tests
cargo test --locked -p hiroz --test queue
cargo test --locked -p hiroz-codegen --test action_nested_deps
```

## WASM constraints and implementation tradeoffs

Use async context/service/receive APIs in browsers. Blocking waits need a compute
worker; synchronous shared locks still require short critical sections and have
no hard scheduling bound. Graph history arrives asynchronously on WASM, so wait
for discovery rather than assuming `build_async` returns a complete graph.
The non-parking shared locks and asynchronous history initialization are choices
in this port. Existing graph wait helpers let callers wait for discovery without
blocking the browser event loop.

Connection loss or tab suspension cannot guarantee cancellation of an autonomous
goal. The demo's native turtlesim checks do not establish native Nav2 or physical
TurtleBot interoperability, and no hard motor-control deadline is claimed.

## General API limitations

Upstream hiroz already uses an unbounded per-goal action feedback receiver; this
branch retains that implementation and public receiver type. It is unrelated to
Zenoh's WASM transport buffers. Drain or drop feedback; topic QoS does not bound
this queue. Bounding it would be a separate API and overflow-policy change.

The async builder added by this branch uses an explicit Zenoh config or built-in
defaults. It does not load native config files or apply the synchronous builder's
Zenoh environment overrides, including when called on native targets. Native
configuration parity is separate from browser support.

Custom action types without `default_result()` receive explicit errors for
unknown/expired results instead of fabricated payloads. This is action-protocol
behavior on both platforms, not a browser restriction.

See the integration repository's
[validation notes](https://github.com/0x53A/hiroz-web/blob/main/docs/reviews/2026-09-23-upstream-cleanup.md)
for fresh results and the remaining scope.
The subsequent [iterative review](https://github.com/0x53A/hiroz-web/blob/main/docs/reviews/2026-09-23-iterative-review.md)
also covers managed-handler unwinding and native async-builder SHM configuration.
