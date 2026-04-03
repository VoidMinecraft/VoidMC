# Events

Void uses two kinds of events:

1. **Semantic events** — High-level game events triggered via `world.trigger()`, handled by Bevy observers.
2. **Packet message events** — Raw packet events dispatched via `world.write_message()`, consumed with `MessageReader<T>`.

## Semantic Events

These are the primary extension points for plugins. They are triggered after the server has processed the corresponding packet and updated ECS state.

### PlayerJoinEvent

```rust
pub struct PlayerJoinEvent {
    pub client_id: u32,
    pub entity: Entity,
}
```

Triggered when a client finishes configuration and transitions to the Play state. At this point all player components are inserted but `PlayerReady` is **not yet set** (the client hasn't sent `PlayerLoaded`).

### PlayerReadyEvent

```rust
pub struct PlayerReadyEvent {
    pub client_id: u32,
    pub entity: Entity,
}
```

Triggered when the client sends `PlayerLoaded`, indicating they have received initial chunks and are ready to play. The `PlayerReady` marker component is inserted just before this event fires. Other players are notified of the new player via this event's observer.

### PlayerQuitEvent

```rust
pub struct PlayerQuitEvent {
    pub client_id: u32,
    pub entity: Entity,
}
```

Triggered when a ready player disconnects. Observers broadcast entity removal and tab-list updates to remaining players. The entity is despawned after the event is fully processed.

### PlayerMoveEvent

```rust
pub struct PlayerMoveEvent {
    pub entity: Entity,
    pub old_x: f64,
    pub old_y: f64,
    pub old_z: f64,
    pub new_x: f64,
    pub new_y: f64,
    pub new_z: f64,
}
```

Triggered when a player's position changes (from `SetPlayerPos` or `SetPlayerPosAndRot` packets). The `Position` component is already updated when this event fires.

### PlayerRotateEvent

```rust
pub struct PlayerRotateEvent {
    pub entity: Entity,
    pub yaw: f32,
    pub pitch: f32,
}
```

Triggered when a player's look direction changes.

### ChatCommandEvent

```rust
pub struct ChatCommandEvent {
    pub entity: Entity,
    pub client_id: u32,
    pub command: String,
    pub args: Vec<String>,
}
```

Triggered after a chat command is dispatched (regardless of whether the command was found). Allows plugins to observe all command usage.

### ChatMessageEvent

```rust
pub struct ChatMessageEvent {
    pub entity: Entity,
    pub client_id: u32,
    pub message: String,
}
```

Triggered when a player sends a chat message (not a command). The message has already been broadcast to all ready players when this event fires.

## Packet Message Events

Every incoming packet is also dispatched as a message event, giving plugins raw access to protocol data:

| Event | Packet Type |
|---|---|
| `HandshakePacketEvent` | `serverbound::HandshakePacket` |
| `StatusPacketEvent` | `serverbound::StatusPacket` |
| `LoginPacketEvent` | `serverbound::LoginPacket` |
| `ConfigurationPacketEvent` | `serverbound::ConfigurationPacket` |
| `PlayPacketEvent` | `serverbound::PlayPacket` |

Each contains `client_id: u32`, `entity: Entity`, and `packet: <PacketType>`.

These are dispatched via `world.write_message()` and can be consumed in systems using `MessageReader<T>`.

## Observing Events in Plugins

### Semantic Events (Observers)

Register an observer function in your plugin:

```rust
use bevy_ecs::prelude::*;
use void::events::PlayerJoinEvent;

fn on_join(event: On<PlayerJoinEvent>) {
    println!("Player joined! client_id={}", event.client_id);
}

// In your plugin:
VoidServer::new(config)
    .add_plugin(|app| {
        app.add_observer(on_join);
    })
```

### Packet Events (Message Reader)

Read raw packet messages in a system:

```rust
use bevy_ecs::prelude::*;
use void::events::PlayPacketEvent;

fn my_packet_system(mut reader: MessageReader<PlayPacketEvent>) {
    for event in reader.read() {
        // Access event.packet, event.client_id, event.entity
    }
}
```

## Event Dispatch Mechanism

Semantic events use Bevy's trigger system:

```rust
world.trigger(PlayerJoinEvent { client_id, entity });
world.flush();
```

The `world.flush()` call ensures all observer side-effects (entity spawns, component insertions, etc.) are applied immediately within the same tick, before subsequent systems run.
