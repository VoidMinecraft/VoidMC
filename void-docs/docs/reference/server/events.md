# Events

Void has one event mechanism: Bevy **observers**. Every event is a plain
struct with `#[derive(Event)]`, fired with `world.trigger(..)` (or
`commands.trigger(..)` from a system/observer) and consumed with an
`On<T>` observer registered through `app.add_observer(..)`.

```rust
use bevy_app::AppExit;
use voidmc::{On, ServerConfig, VoidServer, events::PlayerJoinEvent};

fn on_join(event: On<PlayerJoinEvent>) {
    tracing::info!(client_id = event.client_id, "Player joined");
}

fn main() -> AppExit {
    VoidServer::new(ServerConfig::default())
        .add_plugin(|app| {
            app.add_observer(on_join);
        })
        .run()
}
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
player. Apart from `PlayerTeleportEvent`, none of them derive `Clone` or
`Debug`.

The movement, input and interaction events report what a client sent, at most
one event of each kind per packet the server accepts. They are observations of
client input, not gameplay the server applies: VoidMC has no combat, damage,
health, hunger or mob AI, and an event such as `PlayerInteractEntityEvent`
changes nothing unless your own observer does.

### Observing gameplay from a plugin

An application observes these events with public API only. The event carries
the player's `Entity`; the rest (name, UUID, inventory, world) is read from the
ECS inside the observer:

```rust
use voidmc::components::PlayerName;
use voidmc::events::{BlockPlaceEvent, PlayerMoveEvent, PlayerUseItemEvent};
use voidmc::{Inventory, On, Query, VoidServer};

fn on_move(event: On<PlayerMoveEvent>, names: Query<&PlayerName>) {
    if let Ok(name) = names.get(event.entity) {
        tracing::info!(
            player = %name.0,
            from = ?(event.old_x, event.old_y, event.old_z),
            to = ?(event.new_x, event.new_y, event.new_z),
            "moved"
        );
    }
}

fn on_place(event: On<BlockPlaceEvent>, names: Query<&PlayerName>) {
    if let Ok(name) = names.get(event.entity) {
        tracing::info!(player = %name.0, position = ?event.position, state = event.placed_state, "placed");
    }
}

// The use events carry the hand, not the item: read it from the inventory.
fn on_use(event: On<PlayerUseItemEvent>, inventories: Query<&Inventory>) {
    if let Ok(inventory) = inventories.get(event.entity) {
        let item = match event.hand {
            voidmc::Hand::MainHand => inventory.held(),
            voidmc::Hand::OffHand => inventory.get(Inventory::OFFHAND),
        };
        tracing::info!(item = ?item.item.name(), "used");
    }
}

VoidServer::new(config).add_plugin(|app| {
    app.add_observer(on_move)
        .add_observer(on_place)
        .add_observer(on_use);
});
```

The end-to-end scenario `void-e2e/tests/scenarios/atp_035_gameplay_events.rs`
records the movement, input, chat, block and item events this way while two
clients play, and checks that each action produced exactly its events.

### Connection lifecycle

| Event | Fields | When |
|---|---|---|
| `PlayerJoinEvent` | `client_id`, `entity` | Configuration finished: the client's `FinishConfigurationAcknowledged` moved it to `ConnectionState(Play)`; player components inserted, initial chunks and teleport sent. `PlayerReady` is **not** set yet. Once per session. |
| `PlayerReadyEvent` | `client_id`, `entity` | Client sent `PlayerLoaded`. `PlayerReady` was inserted just before. Built-in observers spawn the player for others, replay entities, sync inventory. Once per session: a repeated `PlayerLoaded` is ignored, and a client that never sends it never becomes ready. |
| `PlayerQuitEvent` | `client_id`, `entity` | A player who joined left, whether or not it became ready. The entity and its components can still be read; `PlayerReady` is present if it was ready. Observers broadcast `RemoveEntities`/`PlayerInfoRemove` for a ready player. The entity is despawned after the event. Once per session. |

Each session (one connection) sees `PlayerJoinEvent`, then `PlayerReadyEvent`
if the client loads, then `PlayerQuitEvent`, each at most once and on the same
entity; a player reconnecting with the same name is a new entity and a new
sequence. A client that leaves during login or configuration fires none of
them. `PlayerQuitEvent` fires however the player leaves:

| Way out | When the quit event fires |
|---|---|
| The client closes the connection, or it drops | When the network thread reports the closed connection, at the next ingest. |
| Kick (`/kick`, `Players::kick`) | After the `Disconnect` is written and the server has closed the connection, even if the client ignores the packet. |
| Keep-alive timeout, outbound queue overflow | Same as a kick. |
| Server shutdown (`/stop`, Ctrl-C, `AppExit`) | In the last tick, for every player still connected. |

A client entity despawned by game code gets no quit event; kick it instead (see
[Kicking a player](./sending-packets.md#kicking-a-player)).

### Movement and input

| Event | Fields | When |
|---|---|---|
| `PlayerMoveEvent` | `entity`, `old_x/y/z`, `new_x/y/z` | One per position packet the server accepts (`SetPlayerPos`, or the position of `SetPlayerPosAndRot`), even when it repeats the current position (`old` = `new`; clients resend it periodically). Absolute block coordinates; `Position` already holds `new_*`. Not fired while the player rides (`Mount`), carries `ServerControlledPosition` or has a teleport the client has not confirmed yet (`TeleportState.pending_id`), whose movement packets are ignored whole, nor for server-side moves (`Teleport`, `/tp`, writing `Position`). |
| `PlayerRotateEvent` | `entity`, `yaw`, `pitch` | One per rotation packet (`SetPlayerRotation`, or the rotation of `SetPlayerPosAndRot`), also unchanged, mounted or server-controlled. Not fired while the player has a teleport the client has not confirmed yet (`TeleportState.pending_id`): like vanilla, its movement packets are ignored whole, rotation included. Degrees exactly as sent: yaw is not wrapped into ±180. `Rotation` already updated. A `SetPlayerPosAndRot` fires a move and a rotate event. |
| `PlayerInputEvent` | `entity`, `forward`, `backward`, `left`, `right`, `jump`, `sneak`, `sprint` | Movement-key state changed; each field is whether the key is held. |
| `PlayerSneakEvent` | `entity`, `sneaking` | The sneak key in Player Input changed (clients report sneaking there since 1.21.2), right after that packet's `PlayerInputEvent`. |
| `PlayerSprintEvent` | `entity`, `sprinting` | Player Command start/stop sprinting. |
| `PlayerToggleFlyEvent` | `entity`, `flying` | Client toggled flight. |
| `PlayerTeleportEvent` | `entity`, `outcome` | A `Teleport` left the player: `Confirmed`, `TimedOut` or `Cancelled` (see [Players](../gameplay/players.md#teleportation)). |

### Chat and commands

| Event | Fields | When |
|---|---|---|
| `ChatMessageEvent` | `entity`, `client_id`, `message` | A chat line not starting with `/`, after `<name> message` was broadcast: an observer sees it but cannot cancel or rewrite it. `message` is the text exactly as sent. |
| `ChatCommandEvent` | `entity`, `client_id`, `command`, `args` | A command was enqueued (found or not), from a command packet or a chat line starting with `/` (which fires no `ChatMessageEvent`). Execution happens later in `VoidSystems::CommandDrain`. |

### Interaction

| Event | Fields | When |
|---|---|---|
| `PlayerSwingArmEvent` | `entity`, `hand` | Arm swing animation. |
| `PlayerInteractEntityEvent` | `entity`, `target_id`, `attack`, `hand`, `target_pos`, `sneaking` | The client attacked an entity (Attack packet) or right-clicked one (Interact packet). An attack has `attack: true`, no `hand`, no `target_pos` and `sneaking: false`, since the packet carries only the target. A right-click with either hand has `attack: false`, the `hand`, the hit position relative to the entity in `target_pos` (sent at low precision), and whether the sneak key was pressed. Input only: nothing consumes it, and no attack, damage or knockback is applied. |
| `PlayerStartDiggingEvent` / `PlayerCancelDiggingEvent` / `PlayerFinishDiggingEvent` | `entity`, `position`, `face`, `sequence` | Block digging state machine, one per packet, whether or not a block is there. Finish (or, in creative, start) is what leads to a break. |
| `PlayerUseItemEvent` | `entity`, `hand`, `sequence` | Right click in the air, whatever the hand holds (an empty hand too). Fired when the packet arrives, before the use runs in `VoidSystems::ItemUseDrain`; the item is not in the payload, read it from `Inventory` (`held()` or `get(Inventory::OFFHAND)`). |
| `PlayerUseItemOnBlockEvent` | `entity`, `hand`, `position`, `face`, `cursor_x/y/z`, `inside_block`, `sequence` | Right click on a block, fired like `PlayerUseItemEvent`. `position` is the clicked block, `face` the clicked face, `cursor_*` the hit point on that face (0 to 1). A placement that follows fires `BlockPlaceEvent`. |
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
| `BlockChangeEvent` | `dimension`, `position`, `old_state`, `new_state`, `source: Option<Entity>` | A block changed through a player break or placement (including `ItemUseContext::place_block`/`set_block`), after the chunk was updated and `BlockUpdate` broadcast: the world already holds `new_state`. Fired just before the matching break or place event. Edits that write chunk data directly (world edit, generators) do not fire it. |
| `BlockBreakEvent` | `entity`, `dimension`, `position`, `broken_state` | A player broke a block; `broken_state` is the block it had, the world already holds air. Not fired for a dig on air or when an item's `before_break_block` keeps the block. |
| `BlockPlaceEvent` | `entity`, `dimension`, `position`, `face`, `placed_state` | A block was placed for a player: `position` is where it went (the clicked block offset by `face`) and the world already holds `placed_state`. Also fired by `ItemUseContext::place_block` and `set_block` (with `face` `Top`). Not fired when nothing changes: an empty hand, an item that is not a block, an unloaded chunk or the same state. |

### World storage

| Event | Fields | When |
|---|---|---|
| `ChunkLoadFailedEvent` | `dimension`, `pos`, `kind: ChunkLoadErrorKind` (`Decode` or `Io`), `error`, `action: DamagedChunkAction` (`Quarantined(path)` or `SavesSkipped`) | A chunk loader found a stored chunk it could not load, and the chunk was generated instead. Triggered by `voidmc-world-io` at the end of the tick in which the chunk was loaded (startup or streaming). See [Damaged chunks](/reference/server/world-serialization#damaged-chunks). |

## Firing events from your code

```rust
world.trigger(EntityDespawnEvent { entity });
world.trigger(ItemDropEvent { dropper, stack });
```

From a system or observer use `commands.trigger(..)`; it is applied at the next
flush. Defining your own event is the same `#[derive(Event)]` + `add_observer`.
