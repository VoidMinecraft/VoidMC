//! UI on the vanilla client, with no mod: dialogs, text that reacts to clicks and
//! hovering, and the resource pack that draws it, generated when the server starts.
//!
//! ```no_run
//! use voidmc_ui::{Button, Dialog, Text};
//!
//! let dialog = Dialog::new(Text::new("Shop"), Button::new("Buy").on_click("shop:buy"))
//!     .body(Text::new("Hover me").on_hover(Text::new("Hello")), 200);
//! ```

pub mod dialog;
pub mod glyph;
pub mod pack;
pub mod pixmap;
pub mod plugin;
pub mod text;

pub use dialog::{Button, Dialog};
pub use glyph::{Cell, Cursor, FontBuilder, Image, Tile, glyph_grid};
pub use pack::{BuiltPack, ResourcePack};
pub use pixmap::{Pixmap, Stamp};
pub use plugin::{PackServer, PackSettled, UiClick, UiPlugin};
pub use text::Text;
