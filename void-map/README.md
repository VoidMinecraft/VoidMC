# Carte (POC)

The server's map in a vanilla dialog for Minecraft Java 26.1.2, with no mod and no
shader, built on `void-ui`.

```bash
cargo run -p voidmc-map
```

Join `127.0.0.1:25565` and accept the resource pack. The map opens by itself, and
`/map` opens it again.

| Action | Effect |
|---|---|
| Click an arrow | Moves by a quarter of the map. |
| Click the map | Centres on that zone of 8x8 cells. |
| Hover a marker | Shows its name, kind and coordinates. |
| Click a marker | Turns it white and shows it in the panel. |
| Retour au centre | Goes back to the origin and clears the selection. |
| Fermer, or Escape | Closes the map. |

## How it works

| File | Role |
|---|---|
| `src/window.rs` | The visible cells, moved on a fixed grid. |
| `src/terrain.rs` | Samples the server's `WorldGenerator` over a window. |
| `src/relief.rs` | Heights and biomes to pixels. |
| `src/markers.rs` | Seeded marker placement. |
| `src/view.rs` | Click ids, a player's view, and the dialog. |
| `src/plugin.rs` | Opens the map and answers clicks. |

The map follows the server's generator: pass another one to
`ServerConfigBuilder::world_generator` and it draws that. `WorldGenerator` does not
expose its water level, so `view::SEA_LEVEL` must match it. `DefaultWorldGenerator`
only has plains, so biome colors only show with a generator that has more.

Each cell is one pixel of `voidmc_ui::Pixmap`, 2 GUI pixels and 4 blocks wide. The
pack never changes: every click sends a new dialog of about 160 KB.

| Variable | Default |
|---|---|
| `VOID_MAP_ADDRESS` | `127.0.0.1:25565` |
| `VOID_MAP_PACK_ADDRESS` | `127.0.0.1:25580` |
| `VOID_MAP_PACK_URL` | `http://<pack address>/map.zip` |
