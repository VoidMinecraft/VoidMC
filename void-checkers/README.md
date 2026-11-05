# Dames (POC)

A solo checkers game on a 3D board for Minecraft Java 26.1.2, with no mod and no
shader. The board and pieces are display entities, and you play with the
crosshair from a fixed seat above the board.

```bash
cargo run -p voidmc-checkers
```

Join `127.0.0.1:25565` and accept the resource pack, which gives the pieces their
3D models. You play both sides, and only the first player gets the seat.

| Action | Effect |
|---|---|
| Look at a piece | It lifts and glows if it can move. |
| Left click | Picks it up. It follows the crosshair, and green squares show legal moves. |
| Look around | The blue ghost shows where the piece would land. |
| Left click on a legal square | Drops it. |
| Left click anywhere else | Sends it back. |
| `/dames` | Restarts the game. |

Rules are English draughts on 8x8: men move and capture forward, kings move one
square in any direction, capturing is compulsory, and promotion ends a chain.

## How it works

| File | Role |
|---|---|
| `src/game.rs` | Rules, with no Minecraft types. |
| `src/geometry.rs` | Board layout, crosshair raycast and jump arcs. |
| `src/plugin.rs` | Scene, clicks, animations and effects. |
| `src/pack.rs` | Small HTTP server for the pack archive. |
| `pack/build_pack.py` | Regenerates `pack/checkers.zip`. |

The client reports the player's rotation every tick, so the server knows the
cursor: it casts a ray from the eye to the board plane. A left click at nothing
always sends an arm swing, which is the click signal. That only holds while no
block is within reach of the crosshair, so the board has no real blocks.

The player rides an invisible display entity, which keeps the eye at a known
position and stops them from walking away. `geometry::seat` derives the seat from
the eye offset of a seated player.

Pieces move by position updates that the client interpolates over 2 ticks. The
pack replaces six dye items with piece models; without it, pieces show as flat
dyes.

| Variable | Default |
|---|---|
| `VOID_CHECKERS_ADDRESS` | `127.0.0.1:25565` |
| `VOID_CHECKERS_PACK_ADDRESS` | `127.0.0.1:25580` |
| `VOID_CHECKERS_PACK_URL` | `http://<pack address>/checkers.zip` |

Set `VOID_CHECKERS_PACK_URL` to an address other players can reach when the
server is not local.
