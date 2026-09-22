# Events

Void has one event mechanism: Bevy **observers**. Every event is a plain
struct with `#[derive(Event)]`, fired with `world.trigger(..)` (or
`commands.trigger(..)` from a system/observer) and consumed with an
`On<T>` observer registered through `app.add_observer(..)`.

```rust
use voidmc::{On, events::PlayerJoinEvent};

fn on_join(event: On<PlayerJoinEvent>) {
    println!("Player joined: client_id={}", event.client_id);
}

VoidServer::new(config).add_plugin(|app| {
    app.add_observer(on_join);
});
```

Observers run synchronously at the trigger site. The framework calls
`world.flush()` after each dispatched packet and after each drained command,
so observer side effects (spawns, inserts, nested triggers) are applied before
the next packet or command is processed.

## Raw packet events

Every decoded serverbound packet is also delivered as
`PacketEvent<T>` where `T` is the packet struct from `voidmc_protocol::serverbound`:

```rust
pub struct PacketEvent<T> {
    pub client_id: u32,
    pub entity: Entity,
    pub packet: T,
}
```

```rust
use voidmc::{On, network::PacketEvent};
use voidmc_protocol::serverbound::ChatMessage;

fn on_chat_packet(event: On<PacketEvent<ChatMessage>>) {
    println!("{}: {}", event.client_id, event.packet.message);
}
```

Dispatch happens in `PreUpdate`, inside the
[`VoidSystems::NetworkIngest`](./sending-packets.md#system-sets) set. `event.entity`
is the client entity, which exists from the first packet onward (handshake
included), so it can be passed straight to [`Players::send`](./sending-packets.md).

There is no `write_message` / `MessageReader` path; the types
`HandshakePacketEvent`, `PlayPacketEvent`, etc. do not exist.

## Semantic events

All events live in `voidmc::events`. Unless stated otherwise they are fired by
the framework after the ECS state has been updated, and `entity` is the acting
player. None of them derive `Clone` or `Debug`.

### Connection lifecycle

| Event | Fields | When |
|---|---|---|
| `PlayerJoinEvent` | `client_id`, `entity` | Configuration finished; player components inserted, initial chunks and teleport sent. `PlayerReady` is **not** set yet. |
| `PlayerReadyEvent` | `client_id`, `entity` | Client sent `PlayerLoaded`. `PlayerReady` was inserted just before. Built-in observers spawn the player for others, replay entities, sync inventory. |
| `PlayerQuitEvent` | `client_id`, `entity` | A ready player disconnected. Observers broadcast `RemoveEntities`/`PlayerInfoRemove`. The entity is despawned after the event. |

### Movement and input

| Event | Fields | When |
|---|---|---|
| `PlayerMoveEvent` | `entity`, `old_x/y/z`, `new_x/y/z` | Position packet received; `Position` already updated. |
| `PlayerRotateEvent` | `entity`, `yaw`, `pitch` | Rotation packet received; `Rotation` already updated. |
| `PlayerInputEvent` | `entity`, `forward`, `backward`, `left`, `right`, `jump`, `sneak`, `sprint` | Movement-key state changed; each field is whether the key is held. |
| `PlayerSneakEvent` | `entity`, `sneaking` | Sneak started/stopped. |
| `PlayerSprintEvent` | `entity`, `sprinting` | Sprint started/stopped. |
| `PlayerToggleFlyEvent` | `entity`, `flying` | Client toggled flight. |
| `PlayerTeleportEvent` | `entity`, `outcome` | A `Teleport` left the player: `Confirmed`, `TimedOut` or `Cancelled` (see [Players](../gameplay/players.md#teleportation)). |

### Chat and commands

| Event | Fields | When |
|---|---|---|
| `ChatMessageEvent` | `entity`, `client_id`, `message` | A non-command chat line, after it was broadcast. |
| `ChatCommandEvent` | `entity`, `client_id`, `command`, `args` | A command was enqueued (found or not). Execution happens later in `VoidSystems::CommandDrain`. |

### Interaction

| Event | Fields | When |
|---|---|---|
| `PlayerSwingArmEvent` | `entity`, `hand` | Arm swing animation. |
| `PlayerInteractEntityEvent` | `entity`, `target_id`, `attack`, `hand`, `target_pos`, `sneaking` | Click on an entity. `attack` is true for a left click. Not consumed by the framework. |
| `PlayerStartDiggingEvent` / `PlayerCancelDiggingEvent` / `PlayerFinishDiggingEvent` | `entity`, `position`, `face`, `sequence` | Block digging state machine. Finish is what leads to a break. |
| `PlayerUseItemEvent` | `entity`, `hand`, `sequence` | Right click in the air. |
| `PlayerUseItemOnBlockEvent` | `entity`, `hand`, `position`, `face`, `cursor_x/y/z`, `inside_block`, `sequence` | Right click on a block. |
| `PlayerChangeSlotEvent` | `entity`, `slot` | Hotbar selection changed. |
| `PlayerSwapHandsEvent` | `entity` | Swap main/off hand. |
| `PlayerDropItemEvent` | `entity`, `drop_stack` | Drop key pressed (`drop_stack` = whole stack). |
| `PlayerCloseContainerEvent` | `entity`, `container_id` | Client closed a container window (raw packet; prefer `MenuClosedEvent`). |

### Items and entities

| Event | Fields | When |
|---|---|---|
| `ItemDropEvent` | `dropper`, `stack` | Request to spawn a dropped item. **Developer code may fire this.** |
| `EntityDespawnEvent` | `entity` | Request to despawn a non-player `SpawnedEntity`; equivalent to `commands.entity(e).despawn()`. Removal itself is ghost-proof: an `On<Remove, SpawnedEntity>` observer sends `RemoveEntities` to every current viewer whichever way the entity goes away. |
| `EntityShownEvent` | `entity`, `viewer` | `viewer` just received `Add Entity` for `entity`. Delivered when the tracker system's commands apply, so both entities exist unless despawned in the same tick. Send per-viewer follow-up packets (metadata, passengers) from an observer. |
| `EntityHiddenEvent` | `entity`, `network_id`, `viewer` | `viewer` just received `Remove Entities` for `entity`. Delivered deferred: on despawn `entity` is already gone and on disconnect `viewer` is; observers must not read components from either and use the ids in the payload. |

### Blocks

| Event | Fields | When |
|---|---|---|
| `BlockChangeEvent` | `dimension`, `position`, `old_state`, `new_state`, `source: Option<Entity>` | Any committed block mutation, after `BlockUpdate` was broadcast. |
| `BlockBreakEvent` | `entity`, `dimension`, `position`, `broken_state` | A player broke a block (also fires `BlockChangeEvent`). |
| `BlockPlaceEvent` | `entity`, `dimension`, `position`, `face`, `placed_state` | A player placed a block (also fires `BlockChangeEvent`). |

## Firing events from your code

```rust
world.trigger(EntityDespawnEvent { entity });
world.trigger(ItemDropEvent { dropper, stack });
```

From a system or observer use `commands.trigger(..)`; it is applied at the next
flush. Defining your own event is the same `#[derive(Event)]` + `add_observer`.
