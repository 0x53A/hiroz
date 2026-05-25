use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use hiroz::context::ZContextBuilder;
use hiroz::Builder;
use hiroz_wasm_demo::messages::RosString;

/// Helper: sleep for ms using setTimeout
async fn sleep_ms(ms: i32) {
    wasm_bindgen_futures::JsFuture::from(js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    }))
    .await
    .unwrap();
}

/// Helper: race a future against a timeout
async fn with_timeout<F, T>(future: F, timeout_ms: i32) -> Option<T>
where
    F: std::future::Future<Output = T>,
{
    let timeout = async {
        sleep_ms(timeout_ms).await;
    };

    futures_lite::future::or(
        async { Some(future.await) },
        async {
            timeout.await;
            None
        },
    )
    .await
}

/// Open a ros-z context via WebSocket, verify session is established.
/// Requires: zenohd -l ws/127.0.0.1:7448
#[wasm_bindgen_test]
async fn rosz_session_open() {
    let mut config = zenoh::Config::default();
    config
        .insert_json5("mode", r#""client""#)
        .expect("set mode");
    config
        .insert_json5("connect/endpoints", r#"["ws/127.0.0.1:7448"]"#)
        .expect("set ws endpoint");
    config
        .insert_json5("scouting/multicast/enabled", "false")
        .expect("disable multicast");

    let ctx = ZContextBuilder::default()
        .with_zenoh_config(config)
        .build_async()
        .await
        .expect("Failed to build ZContext");

    // Verify we can create a node (proves session is working)
    let _node = ctx
        .create_node("wasm_test")
        .build()
        .expect("Failed to create node");

    web_sys::console::log_1(&"ros-z session + node created successfully".into());
}

/// Pub/sub roundtrip using ros-z API with CDR-serialized RosString.
/// This tests the full ros-z stack: context -> node -> publisher/subscriber -> CDR encode/decode.
/// Requires: zenohd -l ws/127.0.0.1:7448
#[wasm_bindgen_test]
async fn rosz_pubsub_roundtrip() {
    let mut config = zenoh::Config::default();
    config
        .insert_json5("mode", r#""client""#)
        .expect("set mode");
    config
        .insert_json5("connect/endpoints", r#"["ws/127.0.0.1:7448"]"#)
        .expect("set ws endpoint");
    config
        .insert_json5("scouting/multicast/enabled", "false")
        .expect("disable multicast");

    let ctx = ZContextBuilder::default()
        .with_zenoh_config(config)
        .build_async()
        .await
        .expect("Failed to build ZContext");

    let node = ctx
        .create_node("wasm_test_node")
        .build()
        .expect("Failed to create node");

    // Create subscriber first
    let sub = node
        .create_sub::<RosString>("wasm_test_topic")
        .build()
        .expect("Failed to create subscriber");

    // Let subscription propagate
    sleep_ms(500).await;

    // Create publisher and publish
    let pub_ = node
        .create_pub::<RosString>("wasm_test_topic")
        .build()
        .expect("Failed to create publisher");

    let msg = RosString {
        data: "Hello from WASM ros-z!".to_string(),
    };
    pub_.async_publish(&msg)
        .await
        .expect("Failed to publish");

    // Receive with timeout
    let received = with_timeout(sub.async_recv(), 5000).await;

    let received = received
        .expect("Timed out waiting for ros-z pub/sub roundtrip")
        .expect("Deserialization failed");
    assert_eq!(received.data, "Hello from WASM ros-z!");

    web_sys::console::log_1(
        &format!("ros-z pub/sub roundtrip OK: received '{}'", received.data).into(),
    );
}
