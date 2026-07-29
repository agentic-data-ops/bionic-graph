#!/bin/bash
# Start cluster: master + 2 workers
set -e

BIN="/tmp/bionic-graph/target/debug/bionic-graph"

echo "Cleaning up old processes..."
pkill -f "bionic-graph.*master" 2>/dev/null || true
pkill -f "bionic-graph.*worker1" 2>/dev/null || true
pkill -f "bionic-graph.*worker2" 2>/dev/null || true
sleep 1

echo "Starting master..."
RUST_LOG=debug $BIN --config ~/.config/bionic-graph/master.json > /tmp/bionic-graph/master.log 2>&1 &
echo "Master PID: $!"

sleep 3

echo "Starting worker1..."
RUST_LOG=debug $BIN --config ~/.config/bionic-graph/worker1.json > /tmp/bionic-graph/worker1.log 2>&1 &
echo "Worker1 PID: $!"

sleep 2

echo "Starting worker2..."
RUST_LOG=debug $BIN --config ~/.config/bionic-graph/worker2.json > /tmp/bionic-graph/worker2.log 2>&1 &
echo "Worker2 PID: $!"

sleep 3

echo "=== Cluster started ==="
echo "Master:  http://127.0.0.1:8080"
echo "Worker1: http://127.0.0.1:8081"
echo "Worker2: http://127.0.0.1:8082"
