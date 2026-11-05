# void-ui

UI on the vanilla client, with no mod and no shader: dialogs, text that reacts to
clicks and hovering, and the resource pack that draws it, generated at startup.
`void-checkers` is a complete example.

## Setup

Draw your pictures into a font, build the pack, serve it, and add the plugin.

```rust
use voidmc_ui::{FontBuilder, Image, PackServer, ResourcePack, Text, UiPlugin};

let mut pack = ResourcePack::new("My UI");
let mut font = FontBuilder::new("myserver", "ui");
let grass = font.tile(Image::filled(18, 18, [80, 160, 60, 255]));
let font_id = font.id();
font.install(&mut pack);

let server = PackServer::start(pack.build(), "0.0.0.0:25580", "http://my.host:25580/ui.zip")?;
app.add_plugins(UiPlugin::new(server, Text::new("This server draws its UI with a pack")));
```

The plugin pushes the pack to each player who joins. `PackSettled` fires once the
client has loaded or refused it.

## Dialogs and clicks

```rust
use voidmc_ui::{Button, Dialog, Text, UiClick};

let dialog = Dialog::new(Text::new("Shop"), Button::new("Buy").on_click("shop:buy"))
    .body(Text::new("Hover me").on_hover(Text::new("Hello")), 200);
players.send(player, dialog.packet());

fn on_click(event: On<UiClick>) {
    tracing::info!(player = ?event.player, id = %event.id, "clicked");
}
```

A dialog stays open across clicks. Send another one to replace it, or
`Dialog::close()` to close it. Add `Dialog::exit` if you need to know when the
player leaves: its click also fires on Escape. Clicking text focuses it, and the
client outlines it in white until the next dialog arrives, so answer every click.

## Glyph grids

`glyph_grid` draws tiles as one text block, and each cell can have its own click
and hover. Tiles only need the same height, a multiple of 9px, because a tile takes
one text line per 9px. Widths can differ, for borders and panels.

```rust
use voidmc_ui::{Cell, glyph_grid};

let map = glyph_grid(&font_id, 8, 8, |column, row| {
    Cell::new(&grass).on_click(format!("map:{column}/{row}"))
});
```

## Following the mouse

The server never sees the mouse. `FontBuilder::cursor` makes a picture for
`on_hover` instead: the client keeps tooltips on the mouse, and the cursor's
advances cancel the tooltip's offset. Call `ResourcePack::hide_tooltip_frames` to
remove the box. That hides the box of every tooltip while the pack is loaded.

Put the same cursor on every cell, so it does not vanish between two of them.

## Limits

- Hovering stays on the client: the server only learns about clicks.
- Each click costs a round trip, and animation means sending new dialogs.
- Grid cell tooltips are per glyph, so a large grid with hovers makes a large packet.
- The pack format is that of Minecraft 26.1.2.
