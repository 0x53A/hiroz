use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use hiroz::msg::ZMessage;
use hiroz::MessageTypeInfo;
use hiroz_wasm_demo::messages::RosString;

/// Verify RosString CDR serialization roundtrip works in WASM.
#[wasm_bindgen_test]
fn cdr_roundtrip() {
    let msg = RosString {
        data: "Hello from WASM!".to_string(),
    };

    let bytes = msg.serialize();
    let decoded = RosString::deserialize(&bytes).expect("CDR deserialize");

    assert_eq!(msg, decoded);
}

/// Verify empty string CDR roundtrip.
#[wasm_bindgen_test]
fn cdr_empty_string() {
    let msg = RosString {
        data: String::new(),
    };

    let bytes = msg.serialize();
    let decoded = RosString::deserialize(&bytes).expect("CDR deserialize empty");

    assert_eq!(msg, decoded);
}

/// Verify MessageTypeInfo returns correct ROS 2 DDS type name.
#[wasm_bindgen_test]
fn message_type_info() {
    assert_eq!(RosString::type_name(), "std_msgs::msg::dds_::String_");
}

/// Verify zenoh config can be created for WASM WebSocket client.
#[wasm_bindgen_test]
fn zenoh_config_for_ws() {
    let mut config = zenoh::Config::default();
    config
        .insert_json5("mode", r#""client""#)
        .expect("set mode");
    config
        .insert_json5("connect/endpoints", r#"["ws/127.0.0.1:7448"]"#)
        .expect("set ws endpoint");
}

/// Verify ros-z key expression generation for rmw_zenoh format.
#[wasm_bindgen_test]
fn rmw_zenoh_key_expression() {
    use hiroz_protocol::format::KeyExprFormat;
    use hiroz_protocol::entity::*;

    let zid: zenoh::session::ZenohId = "1234567890abcdef1234567890abcdef".parse().unwrap();

    let node = NodeEntity {
        domain_id: 0,
        z_id: zid,
        id: 1,
        name: "wasm_node".to_string(),
        namespace: String::new(),
        enclave: String::new(),
    };

    let entity = EndpointEntity {
        id: 1,
        node: Some(node.clone()),
        kind: EndpointKind::Publisher,
        topic: "chatter".to_string(),
        type_info: Some(TypeInfo {
            name: "std_msgs::msg::dds_::String_".to_string(),
            hash: hiroz_protocol::entity::TypeHash::zero(),
        }),
        qos: Default::default(),
    };

    let ke = KeyExprFormat::RmwZenoh
        .topic_key_expr(&entity)
        .expect("topic key expr");

    let ke_str = ke.as_str();
    // Should be: 0/chatter/std_msgs::msg::dds_::String_/RIHS01_0000...
    assert!(ke_str.starts_with("0/chatter/"), "key expr should start with domain/topic, got: {}", ke_str);
    assert!(ke_str.contains("std_msgs::msg::dds_::String_"), "key expr should contain type name, got: {}", ke_str);
}

/// Verify ros-z ZContextBuilder can be created (no actual connection).
#[wasm_bindgen_test]
fn context_builder_creation() {
    use hiroz::context::ZContextBuilder;

    // Just verify the builder can be created without panicking
    let _builder = ZContextBuilder::default();
}
