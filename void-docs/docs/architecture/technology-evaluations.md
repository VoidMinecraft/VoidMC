# Technology Evaluations

As part of the technical track for the End-of-Studies Project (EIP), developing a high-performance Minecraft server from scratch requires careful evaluation of foundational technologies. This document outlines the rationale behind our core technology choices, fulfilling the objective of evaluating and integrating new technologies ("veille technologique").

## Custom Codec vs. Serde

When building the Minecraft protocol layer, we had to decide how to handle encoding and decoding network packets. Serde is the widely accepted standard for serialization and deserialization in Rust, but we opted to build a custom solution: `void-codec`.

### The Problem with Serde
Minecraft's protocol heavily relies on domain-specific data types, most notably variable-length integers (`VarInt` and `VarLong`). Serde's data model is primarily designed for self-describing formats (like JSON) or standard binary formats. It maps cleanly to standard integer primitive types, but it does not easily support distinguishing between a standard integer and a variable-length integer natively within its data model.

Attempting to implement this with Serde required awkward workarounds, such as explicitly handling variable length types as raw byte buffers (`[u8]`) rather than strong numerical types.

### Our Solution
We developed `void-codec` and `void-codec-macros` to provide an exact fit for the Minecraft protocol specifications.
- **Type-Safety:** It allows us to define Minecraft-specific primitives and map them exactly how they are structured over the wire.
- **Performance:** Bypassing Serde's intermediate data model removes unnecessary abstractions, directly converting packet structures into raw bytes.
- **Ergonomics:** Through custom procedural macros, defining a packet is as simple as defining a Rust `struct` without needing complex Serde annotations to handle edge cases like `VarInt`.

## Tokio Runtime

A Minecraft server is a highly concurrent, I/O-bound application that must maintain simultaneous TCP connections with potentially thousands of players while continuing to process game ticks smoothly.

### Rationale for Tokio
We chose [Tokio](https://tokio.rs/) as our asynchronous runtime for the `void-net` layer:

1. **Industry Standard:** Tokio is the most mature, active, and battle-tested async runtime in the Rust ecosystem. Its extensive documentation and community support significantly reduce development friction.
2. **High-Concurrency I/O:** Tokio's event loop (built on `epoll`) is extremely efficient at handling thousands of inactive or minimally active TCP sockets, which is perfect for maintaining player connections without blocking the main thread.
3. **Architectural Fit:** Our server employs a dual-threaded model: a Tokio-driven network thread pool and a Bevy ECS-driven game thread. Tokio facilitates lightweight, non-blocking network operations and seamlessly integrates with flume channels to pass decoded packets safely to the Bevy ECS world without locking.

## AI Agents Integration & Contextualization

As part of our continuous technology watch ("veille technologique"), we actively explored how Large Language Models (LLMs) and autonomous coding agents could be integrated directly into our development workflow to accelerate prototyping and refactoring.

### The Problem with Generic AI
While standard AI assistants are effective for boilerplate code, they struggle with heavily opinionated, custom architectures like ours (a dual-threaded Tokio + Bevy ECS environment). Generic tools often hallucinate non-idiomatic code or suggest standard patterns that violate our strict separation of concerns (e.g., trying to use Mutex locks instead of `flume` channels).

### Our Solution: Automated Workspace Rules (`AGENTS.md`)
To solve this, we implemented AI contextualization directly into the repository. We created an `AGENTS.md` (and related configuration files) to serve as a ground-truth "Runbook" and "Architecture Note" for AI agents operating in the workspace.

- **Workspace Map:** Forces the AI to understand our mult-crate setup (`void`, `void-net`, `void-codec`, etc.) before generating code.
- **Architectural Rules:** Explicitly instructs the AI that the runtime is dual-threaded and that cross-thread communication must use `flume`.
- **Change Patterns:** Guides the AI on how to correctly structure a new feature (e.g., adding a plugin module, registering it in `DefaultPlugins`).

**Impact:** Incorporating contextualized AI agents transformed them from simple autocomplete tools into reliable project contributors. By maintaining the `AGENTS.md` file, we significantly reduced the "hallucination rate" of AI tools when generating Bevy systems or Tokio protocols, acting as a successful integration of an emerging modern technology into our daily engineering process.
