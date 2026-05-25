#!/bin/bash
set -e

# Source ROS 2 Jazzy
source /opt/ros/jazzy/setup.bash
# Source the rmw_zenoh workspace overlay
source /ros2_ws/install/setup.bash

export RMW_IMPLEMENTATION=rmw_zenoh_cpp
export ZENOH_SESSION_CONFIG_URI=/config/zenoh_session.json5

echo "Starting ROS 2 demo nodes with rmw_zenoh..."
echo "  RMW_IMPLEMENTATION=$RMW_IMPLEMENTATION"
echo "  ZENOH_SESSION_CONFIG_URI=$ZENOH_SESSION_CONFIG_URI"

# Run talker and listener
ros2 run demo_nodes_cpp talker &
TALKER_PID=$!

ros2 run demo_nodes_cpp listener &
LISTENER_PID=$!

echo "Talker PID=$TALKER_PID, Listener PID=$LISTENER_PID"

# Wait for either to exit
wait -n $TALKER_PID $LISTENER_PID
