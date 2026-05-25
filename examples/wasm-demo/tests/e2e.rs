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

/// End-to-end test: Subscribe to /chatter from a ROS 2 Jazzy talker via rmw_zenoh.
///
/// Requires:
///   - zenoh router on ws/127.0.0.1:7448
///   - ROS 2 Jazzy talker running with rmw_zenoh_cpp connected to the same router
///
/// The talker publishes std_msgs/msg/String on /chatter.
/// We subscribe using ros-z with the rmw-zenoh key expression format.
#[wasm_bindgen_test]
async fn subscribe_to_ros2_chatter() {
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
        .create_node("wasm_e2e_listener")
        .build()
        .expect("Failed to create node");

    // Subscribe to /chatter topic
    let sub = node
        .create_sub::<RosString>("chatter")
        .build()
        .expect("Failed to subscribe to chatter");

    web_sys::console::log_1(&"Waiting for messages from ROS 2 talker on /chatter...".into());

    // Wait up to 15 seconds for a message from the ROS 2 talker (publishes every 1s)
    let received = with_timeout(sub.async_recv(), 15000).await;

    let msg = received
        .expect("Timed out waiting for ROS 2 talker message on /chatter")
        .expect("Failed to deserialize ROS 2 message");

    web_sys::console::log_1(
        &format!("Received from ROS 2 talker: '{}'", msg.data).into(),
    );

    // The demo talker sends "Hello World: <N>"
    assert!(
        msg.data.starts_with("Hello World:"),
        "Expected 'Hello World: ...' from ROS 2 talker, got: '{}'",
        msg.data
    );
}

/// End-to-end test: Publish to /chatter so the ROS 2 listener receives it.
///
/// Requires same setup as subscribe_to_ros2_chatter.
/// Verification is implicit (the ROS 2 listener will log the message).
#[wasm_bindgen_test]
async fn publish_to_ros2_chatter() {
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
        .create_node("wasm_e2e_talker")
        .build()
        .expect("Failed to create node");

    let pub_ = node
        .create_pub::<RosString>("chatter")
        .build()
        .expect("Failed to create publisher");

    // Small delay for publisher to register
    sleep_ms(500).await;

    let msg = RosString {
        data: "Hello from WASM ros-z browser!".to_string(),
    };

    pub_.async_publish(&msg)
        .await
        .expect("Failed to publish to chatter");

    web_sys::console::log_1(
        &format!("Published to /chatter: '{}'", msg.data).into(),
    );

    // If we get here without error, the publish succeeded.
    // The ROS 2 listener should print our message in its logs.
}
