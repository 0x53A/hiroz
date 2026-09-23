//! Convenience re-exports for common hiroz types.
//!
//! Import everything with `use hiroz::prelude::*;` to get the types and traits
//! needed for most use cases without hunting through submodules.
//!
//! # Example
//!
//! ```rust,ignore
//! use hiroz::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let ctx = ZContextBuilder::default().build()?;
//!     let node = ctx.create_node("my_node").build()?;
//!     // ...
//!     Ok(())
//! }
//! ```

/// The `Result` alias used throughout hiroz (equivalent to `zenoh::Result`).
pub use zenoh::Result;

/// The builder trait — required to call `.build()` on any builder type.
pub use crate::Builder;
/// Action types.
pub use crate::action::ClientGoalHandle;
/// Action type marker trait.
pub use crate::action::ZAction;
pub use crate::action::server::{Accepted, Executing, Requested};
/// Cache subscriber: retains a sliding window of messages indexed by time.
pub use crate::cache::{ExtractorStamp, ZCache, ZCacheBuilder, ZenohStamp};
/// Core runtime types.
pub use crate::context::{ZContext, ZContextBuilder};
/// Type identity helpers for custom message definitions.
pub use crate::entity::{TypeHash, TypeInfo};
/// Lifecycle node support.
pub use crate::lifecycle::{
    CallbackReturn, LifecycleState, ManagedEntity, ZLifecycleClient, ZLifecycleNode,
    ZLifecyclePublisher,
};
/// Parameter types for ROS 2-compatible node parameters.
pub use crate::parameter::{
    FloatingPointRange, IntegerRange, Parameter, ParameterClient, ParameterDescriptor,
    ParameterList, ParameterTarget, ParameterType, ParameterValue, SetParametersResult,
};
/// QoS configuration types.
pub use crate::qos::{
    QosDurability, QosDuration, QosHistory, QosLiveliness, QosProfile, QosReliability,
};
/// Trait bounds for custom messages and services.
pub use crate::ros_msg::{ActionTypeInfo, MessageTypeInfo, ServiceTypeInfo, WithTypeInfo};
/// Time and clock support.
pub use crate::time::{ClockKind, ZClock, ZDuration, ZInterval, ZSleep, ZTime};
pub use crate::{
    action::{GoalId, GoalStatus},
    node::ZNode,
    pubsub::{ZPub, ZSub},
    service::{RequestId, ServiceReply, ServiceRequest, ZClient, ZServer},
};
