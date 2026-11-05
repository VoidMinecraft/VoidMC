# Dames

A solo checkers demo for Minecraft Java 26.1.2, with no mod and no shader. The 2.5D
board is a dialog built with [`void-ui`](../void-ui/README.md), and its art is
drawn when the server starts.

```bash
cargo run -p voidmc-checkers
```

Join `127.0.0.1:25565` and accept the resource pack, which this demo requires. The
board opens by itself, and `/dames` reopens it. You play both sides: click a piece,
move the mouse, click a green dot to drop it. Clicking the hollow it left puts it
back.

Each side has a clock that runs on its turn. The trays hold the pieces each side
has taken: black pieces on the left, white ones on the right.

Rules are English draughts on 8x8: men move and capture forward, kings move one
square in any direction, capturing is compulsory, and promotion ends a chain.

| File | Role |
|---|---|
| `src/game.rs` | Rules, with no Minecraft types. |
| `src/art.rs` | Draws the tiles and adds them to the pack. |
| `src/board.rs` | Clocks, and game state to dialog. |
| `src/plugin.rs` | Sessions, clicks, the once-a-second refresh, `/dames`. |

| Variable | Default |
|---|---|
| `VOID_CHECKERS_ADDRESS` | `127.0.0.1:25565` |
| `VOID_CHECKERS_PACK_ADDRESS` | `127.0.0.1:25580` |
| `VOID_CHECKERS_PACK_URL` | `http://<pack address>/checkers.zip` |
