//! hiroz (ros-z) on the multi-threaded WASM runtime, talking to ROS 2.
//!
//! Architecture: the whole ROS stack (zenoh session, node, subscriber,
//! publisher) lives in one task on the Application worker of the
//! SharedArrayBuffer threadpool. The browser main thread communicates with it
//! exclusively through flume channels: incoming /chatter samples flow out via
//! `ros_poll()`, outgoing messages flow in via `ros_publish()`.

use std::sync::OnceLock;

use wasm_bindgen::prelude::*;

use hiroz_msgs::std_msgs::String as RosString;

/// Samples received from ROS 2 (/chatter), drained by the main thread.
static FROM_ROS: OnceLock<flume::Receiver<String>> = OnceLock::new();
/// Messages the main thread wants published to /chatter.
static TO_ROS: OnceLock<flume::Sender<String>> = OnceLock::new();
/// Status/error lines from the worker task.
static STATUS: OnceLock<flume::Receiver<String>> = OnceLock::new();

/// Initialize the threaded runtime. Returns false if SharedArrayBuffer is
/// unavailable (missing COOP/COEP headers).
#[wasm_bindgen]
pub fn ros_start(shim_url: &str) -> bool {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&JsValue::from_str(&format!("PANIC: {info}")));
    }));
    tracing_wasm::set_as_global_default_with_config(
        tracing_wasm::WASMLayerConfigBuilder::new()
            .set_max_level(tracing::Level::INFO)
            .set_console_config(tracing_wasm::ConsoleConfig::ReportWithoutConsoleColor)
            .build(),
    );
    zenoh_runtime::__zenoh_init_threaded_runtime(shim_url)
}

/// Spawn the ROS worker task on the Application worker. Call once, after
/// `ros_start` returned true.
#[wasm_bindgen]
pub fn ros_connect(router_endpoint: String) {
    let (from_tx, from_rx) = flume::unbounded::<String>();
    let (to_tx, to_rx) = flume::unbounded::<String>();
    let (status_tx, status_rx) = flume::unbounded::<String>();
    FROM_ROS.set(from_rx).expect("ros_connect called twice");
    TO_ROS.set(to_tx).expect("ros_connect called twice");
    STATUS.set(status_rx).expect("ros_connect called twice");

    // Trampoline: the outer future only captures Send channel endpoints; the
    // hiroz/zenoh objects are created inside the worker and never cross
    // threads, so they don't need to be Send.
    zenoh_runtime::ZRuntime::Application.spawn(async move {
        zenoh_runtime::spawn_on_current(async move {
            let status_tx2 = status_tx.clone();
            match ros_worker(from_tx, to_rx, status_tx, router_endpoint).await {
                Ok(()) => {}
                Err(e) => {
                    let _ = status_tx2.send(format!("ERROR: {e}"));
                }
            }
        });
    });
}

/// The ROS side, running entirely on the Application worker.
async fn ros_worker(
    from_tx: flume::Sender<String>,
    to_rx: flume::Receiver<String>,
    status_tx: flume::Sender<String>,
    router_endpoint: String,
) -> Result<(), String> {
    use hiroz::context::ZContextBuilder;
    use hiroz::Builder;

    let mut config = zenoh::Config::default();
    config
        .insert_json5("mode", r#""client""#)
        .map_err(|e| format!("config mode: {e}"))?;
    config
        .insert_json5("connect/endpoints", &format!(r#"["{router_endpoint}"]"#))
        .map_err(|e| format!("config endpoints: {e}"))?;
    config
        .insert_json5("scouting/multicast/enabled", "false")
        .map_err(|e| format!("config scouting: {e}"))?;

    let ctx = ZContextBuilder::default()
        .with_zenoh_config(config)
        .build_async()
        .await
        .map_err(|e| format!("ZContext: {e}"))?;
    let node = ctx
        .create_node("wasm_threaded_browser")
        .build()
        .map_err(|e| format!("node: {e}"))?;
    let sub = node
        .create_sub::<RosString>("chatter")
        .build()
        .map_err(|e| format!("subscriber: {e}"))?;
    let publisher = node
        .create_pub::<RosString>("chatter")
        .build()
        .map_err(|e| format!("publisher: {e}"))?;

    let _ = status_tx.send("CONNECTED".to_string());

    // Serve both directions: incoming /chatter samples and outgoing publishes.
    enum Ev {
        FromRos(Result<RosString, String>),
        ToRos(Result<String, flume::RecvError>),
    }
    loop {
        let recv_ros = async { Ev::FromRos(sub.async_recv().await.map_err(|e| e.to_string())) };
        let recv_ui = async { Ev::ToRos(to_rx.recv_async().await) };
        match futures_race(recv_ros, recv_ui).await {
            Ev::FromRos(Ok(msg)) => {
                let _ = from_tx.send(msg.data);
            }
            Ev::FromRos(Err(e)) => {
                let _ = status_tx.send(format!("ERROR: recv: {e}"));
            }
            Ev::ToRos(Ok(text)) => {
                if let Err(e) = publisher.async_publish(&RosString { data: text }).await {
                    let _ = status_tx.send(format!("ERROR: publish: {e}"));
                }
            }
            Ev::ToRos(Err(_)) => break, // UI channel closed
        }
    }
    Ok(())
}

/// Race two futures, returning the first result. Minimal `select` without
/// extra dependencies; both futures are dropped afterwards, which is fine
/// here because flume/hiroz receivers keep pending items internally.
async fn futures_race<T>(
    a: impl std::future::Future<Output = T>,
    b: impl std::future::Future<Output = T>,
) -> T {
    use std::pin::pin;
    use std::task::Poll;
    let mut a = pin!(a);
    let mut b = pin!(b);
    std::future::poll_fn(move |cx| {
        if let Poll::Ready(v) = a.as_mut().poll(cx) {
            return Poll::Ready(v);
        }
        if let Poll::Ready(v) = b.as_mut().poll(cx) {
            return Poll::Ready(v);
        }
        Poll::Pending
    })
    .await
}

/// Drain one received /chatter message, or null.
#[wasm_bindgen]
pub fn ros_poll() -> JsValue {
    match FROM_ROS.get().and_then(|rx| rx.try_recv().ok()) {
        Some(s) => JsValue::from_str(&s),
        None => JsValue::NULL,
    }
}

/// Drain one status line ("CONNECTED" or "ERROR: ..."), or null.
#[wasm_bindgen]
pub fn ros_poll_status() -> JsValue {
    match STATUS.get().and_then(|rx| rx.try_recv().ok()) {
        Some(s) => JsValue::from_str(&s),
        None => JsValue::NULL,
    }
}

/// Publish a std_msgs/String to /chatter.
#[wasm_bindgen]
pub fn ros_publish(text: String) -> bool {
    TO_ROS.get().map(|tx| tx.send(text).is_ok()).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Headless test entry point
// ---------------------------------------------------------------------------

fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        if let Some(output) = document.get_element_by_id("output") {
            let current = output.inner_html();
            output.set_inner_html(&format!("{current}<p>{msg}</p>"));
        }
    }
}

async fn js_sleep(ms: i32) {
    let _ = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    }))
    .await;
}

/// Automated test: runs on the main thread, drives the worker through the
/// channel API exactly like the interactive page does.
#[wasm_bindgen]
pub async fn run_threaded_ros_test() {
    log("=== hiroz threaded WASM <-> ROS 2 test ===");

    if !ros_start("./pkg/hiroz_wasm_demo.js") {
        log("FAIL: SharedArrayBuffer not available (COOP/COEP headers missing?)");
        log("=== Tests complete ===");
        return;
    }
    log("Threaded runtime initialized (5 workers)");
    js_sleep(500).await;

    ros_connect("ws/127.0.0.1:7448".to_string());

    // Wait for CONNECTED
    log("Test 1: hiroz context + node on threadpool...");
    let mut connected = false;
    for _ in 0..100 {
        match ros_poll_status().as_string().as_deref() {
            Some("CONNECTED") => {
                connected = true;
                break;
            }
            Some(err) => {
                log(&format!("  FAIL: {err}"));
                log("=== Tests complete ===");
                return;
            }
            None => js_sleep(100).await,
        }
    }
    if !connected {
        log("  FAIL: timed out waiting for session (is the docker stack up?)");
        log("=== Tests complete ===");
        return;
    }
    log("  PASS: connected, node + pub/sub declared on worker");

    // Test 2: receive from the ROS 2 Jazzy talker (publishes every 1s)
    log("Test 2: receive /chatter from ROS 2 talker...");
    let mut got: Option<String> = None;
    for _ in 0..150 {
        if let Some(s) = ros_poll().as_string() {
            got = Some(s);
            break;
        }
        js_sleep(100).await;
    }
    match got {
        Some(s) if s.starts_with("Hello World:") => {
            log(&format!("  PASS: received from ROS 2 talker: '{s}'"))
        }
        Some(s) => log(&format!("  FAIL: unexpected payload: '{s}'")),
        None => log("  FAIL: no message from ROS 2 talker within 15s"),
    }

    // Test 3: publish to /chatter; the ROS 2 listener logs it (verified by
    // the host-side runner via docker logs). Our own subscriber also matches,
    // so we can at least confirm the publish left this session.
    log("Test 3: publish to /chatter for the ROS 2 listener...");
    let marker = "Hello from threaded WASM hiroz!";
    ros_publish(marker.to_string());
    let mut echoed = false;
    for _ in 0..50 {
        if let Some(s) = ros_poll().as_string() {
            if s == marker {
                echoed = true;
                break;
            }
        }
        js_sleep(100).await;
    }
    if echoed {
        log("  PASS: publish sent (echoed back through the router)");
    } else {
        log("  WARN: publish not echoed back within 5s (check listener logs)");
    }

    log("=== Tests complete ===");
}
