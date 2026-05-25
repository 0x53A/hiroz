use hiroz::entity::TypeHash;
use hiroz::msg::{SerdeCdrSerdes, ZMessage};
use hiroz::{MessageTypeInfo, WithTypeInfo};
use serde::{Deserialize, Serialize};

/// Manual definition of std_msgs/msg/String for WASM
/// (avoids pulling in ros-z-codegen/ros-z-msgs build pipeline)
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RosString {
    pub data: String,
}

impl MessageTypeInfo for RosString {
    fn type_name() -> &'static str {
        "std_msgs::msg::dds_::String_"
    }

    fn type_hash() -> TypeHash {
        // RIHS01 hash for std_msgs/msg/String in ROS 2 Jazzy
        // Obtained via: ros2 topic info /chatter --verbose
        TypeHash::from_rihs_string(
            "RIHS01_df668c740482bbd48fb39d76a70dfd4bd59db1288021743503259e948f6b1a18",
        )
        .expect("valid RIHS01 hash")
    }
}

impl WithTypeInfo for RosString {}

impl ZMessage for RosString {
    type Serdes = SerdeCdrSerdes<RosString>;
}
