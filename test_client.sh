#!/bin/bash

# Test script for LAN data client

# Clean up any previous test processes
pkill -f "cargo run -- master" || true
pkill -f "cargo run -- slave" || true

echo "Starting master client..."
cargo run -- master ./config.json &
MASTER_PID=$!

sleep 2

echo "Starting slave client..."
cargo run -- slave 8080 mysecretkey &
SLAVE_PID=$!

echo "Test started. Master PID: $MASTER_PID, Slave PID: $SLAVE_PID"

# Wait for 10 seconds
sleep 10

# Cleanup
pkill -f "cargo run -- master" || true
pkill -f "cargo run -- slave" || true

echo "Test completed."
