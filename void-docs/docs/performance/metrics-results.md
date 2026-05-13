# Performance Metrics & Results

As part of our commitment to building a high-performance server (fulfilling the "Mesurer, tester et optimiser les performances techniques" track objective), we have defined key performance indicators (KPIs) and implemented stress-testing procedures.

## Key Performance Indicators (KPIs)

To ensure the server remains stable and fast under load, we monitor the following metrics:

1. **Tick Time (MSPT - Milliseconds Per Tick)**
   * **Target:** < 50ms (Allows the server to maintain a stable 20 Ticks Per Second)
   * **Description:** The time it takes for the Bevy ECS game loop to process a single logical update (game logic, physics, entity updates, player movements).

2. **Memory Footprint per Player**
   * **Target:** < 5 MB per active connection
   * **Description:** The amortized amount of RAM allocated when a struct `Client` is spawned and associated network buffers are created.

3. **Codec Latency**
   * **Target:** < 1ms per packet batch
   * **Description:** The time it takes our custom `void-codec` to serialize/deserialize packet buffers on the Tokio network thread before passing them to the Bevy game thread via Flume channels.

## Automated & Manual Testing Strategy

### 1. Load Testing ("Stress-Test")
We utilize dummy client generators attempting to connect concurrently to measure how our Tokio runtime handles peak connection limits (`epoll` saturation) and how the Bevy ECS scales with lots of empty Entities.

* **Scenario:** 1000 simultaneous connections joining at once.
* **Observation:** The Tokio network thread successfully holds the 1000 connections with minimal CPU overhead. Bevy's ECS handles 1000 `Player` components effortlessly.

### 2. Bottleneck Analysis & Optimizations
**Before Optimization:**
Using generic locking (Mutext/RwLock) to transfer packets from network to game thread caused severe synchronization contention under high load.
**After Optimization:**
Implementing lock-free channels (`flume`) strictly separates the network I/O from the game loop block. The thread synchronization times dropped significantly, maintaining the Tick Time (MSPT) under the 50ms threshold even with heavy I/O.

### 3. Continuous Profiling
Developers can use `cargo bench` to assert the encoding and decoding speeds of typical packets (like `ChunkData`), ensuring new protocol additions do not regress the Codec Latency metric.
