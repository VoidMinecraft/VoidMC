//! Draws a player's view of the map as a dialog: the terrain with its markers,
//! next to a panel of coordinates, arrows and the selected marker.

use std::num::{NonZeroU8, NonZeroU16};

use bevy_ecs::prelude::*;
use voidmc::{TextColor, WorldGenerator};
use voidmc_ui::{Button, Dialog, FontBuilder, Pixmap, ResourcePack, Stamp, Text};

use crate::markers::{Marker, MarkerKind, visible};
use crate::relief::{Style, render};
use crate::terrain::Terrain;
use crate::window::{Direction, Window};

const NAMESPACE: &str = "voidmc:map";

/// `WorldGenerator` does not expose its water level. This is the one of
/// `DefaultWorldGenerator`, and of vanilla.
pub const SEA_LEVEL: i32 = 62;

const COLS: NonZeroU16 = NonZeroU16::new(96).unwrap();
const ROWS: NonZeroU16 = NonZeroU16::new(72).unwrap();
const BLOCKS_PER_CELL: NonZeroU8 = NonZeroU8::new(4).unwrap();
/// Sizes in GUI pixels. The block is 320 wide and, with the button under it, 183
/// tall. On a GUI under 249 tall, the dialog scrolls.
const CELL: u32 = 2;
const STAMP: u32 = 7;
const GAP: u32 = 8;
const PANEL: u32 = 104;
const MAP_WIDTH: u32 = COLS.get() as u32 * CELL;
/// Dialog text wraps at its width, so it needs a margin past the block.
const BODY_WIDTH: i32 = (MAP_WIDTH + GAP + PANEL + 16) as i32;
/// Cells per side of a click zone: one click event each, not one per cell.
const ZONE: u32 = 8;
const BUTTON_WIDTH: i32 = 120;

const SELECTED: [u8; 3] = [255, 255, 255];

fn stamp_color(kind: MarkerKind) -> [u8; 3] {
    match kind {
        MarkerKind::Village => [255, 214, 64],
        MarkerKind::Ruins => [214, 130, 255],
        MarkerKind::Mine => [255, 112, 56],
        MarkerKind::Lighthouse => [96, 240, 232],
    }
}

/// What a player can ask for by clicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pan(Direction),
    /// Centres on a cell of the view.
    Go(u16, u16),
    Select(usize),
    Home,
    Close,
}

impl Action {
    pub fn id(self) -> String {
        let path = match self {
            Action::Pan(Direction::North) => "pan/north".into(),
            Action::Pan(Direction::South) => "pan/south".into(),
            Action::Pan(Direction::East) => "pan/east".into(),
            Action::Pan(Direction::West) => "pan/west".into(),
            Action::Go(col, row) => format!("go/{col}/{row}"),
            Action::Select(index) => format!("marker/{index}"),
            Action::Home => "home".into(),
            Action::Close => "close".into(),
        };
        format!("{NAMESPACE}/{path}")
    }

    pub fn parse(id: &str) -> Option<Self> {
        let path = id.strip_prefix(NAMESPACE)?.strip_prefix('/')?;
        match path {
            "pan/north" => Some(Action::Pan(Direction::North)),
            "pan/south" => Some(Action::Pan(Direction::South)),
            "pan/east" => Some(Action::Pan(Direction::East)),
            "pan/west" => Some(Action::Pan(Direction::West)),
            "home" => Some(Action::Home),
            "close" => Some(Action::Close),
            _ => {
                if let Some(index) = path.strip_prefix("marker/") {
                    return index.parse().ok().map(Action::Select);
                }
                let (col, row) = path.strip_prefix("go/")?.split_once('/')?;
                Some(Action::Go(col.parse().ok()?, row.parse().ok()?))
            }
        }
    }
}

/// What one player is looking at.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct View {
    window: Window,
    selected: Option<usize>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            window: Window::new(0, 0, COLS, ROWS, BLOCKS_PER_CELL),
            selected: None,
        }
    }
}

impl View {
    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Applies a click. Stale or forged ids change nothing: they come from the
    /// client.
    pub fn apply(&mut self, action: Action, markers: &[Marker]) {
        match action {
            Action::Pan(direction) => self.window.pan(direction),
            Action::Go(col, row) => {
                self.window.recenter(col, row);
            }
            Action::Select(index) if index < markers.len() => self.selected = Some(index),
            Action::Select(_) | Action::Close => {}
            Action::Home => *self = Self::default(),
        }
    }
}

/// The glyphs of the map, and how it turns terrain into colors.
#[derive(Resource)]
pub struct MapArt {
    pixmap: Pixmap,
    style: Style,
}

impl MapArt {
    /// Adds the map's font to the pack.
    pub fn new(pack: &mut ResourcePack) -> Self {
        let mut font = FontBuilder::new("voidmc", "map");
        let pixmap = Pixmap::new(&mut font, CELL, u32::from(ROWS.get()), STAMP);
        font.install(pack);
        Self {
            pixmap,
            style: Style::new(SEA_LEVEL),
        }
    }
}

fn hover(marker: &Marker) -> Text {
    Text::new(marker.name.clone())
        .color(TextColor::Gold)
        .append(
            Text::new(format!(
                "\n{} - {}, {}",
                marker.kind.label(),
                marker.x,
                marker.z
            ))
            .color(TextColor::Gray),
        )
}

fn stamps(view: &View, markers: &[Marker]) -> Vec<Stamp> {
    visible(markers, &view.window)
        .map(|(index, col, row)| {
            let marker = &markers[index];
            let color = if view.selected == Some(index) {
                SELECTED
            } else {
                stamp_color(marker.kind)
            };
            Stamp::new(u32::from(col), u32::from(row), color)
                .on_click(Action::Select(index).id())
                .on_hover(hover(marker))
        })
        .collect()
}

fn arrow(label: &str, direction: Direction, tip: &str) -> Text {
    Text::new(label)
        .color(TextColor::Aqua)
        .on_click(Action::Pan(direction).id())
        .on_hover(Text::new(tip))
}

fn details(marker: Option<&Marker>) -> Vec<Text> {
    let Some(marker) = marker else {
        return vec![
            Text::new("Clique un marqueur").color(TextColor::Gray),
            Text::new("ou la carte pour").color(TextColor::Gray),
            Text::new("la recentrer").color(TextColor::Gray),
        ];
    };
    vec![
        Text::new(marker.name.clone()).color(TextColor::Gold),
        Text::new(marker.kind.label()),
        Text::new(format!("X {}  Z {}", marker.x, marker.z)).color(TextColor::Gray),
        Text::new(format!("Altitude {}", marker.height)).color(TextColor::Gray),
    ]
}

fn panel(view: &View, markers: &[Marker], center_height: i32) -> Vec<Text> {
    let (x, z) = view.window.center();
    let shown = visible(markers, &view.window).count();
    let mut lines = vec![
        Text::new(format!("X {x}  Z {z}")),
        Text::new(format!("Altitude {center_height}")).color(TextColor::Gray),
        Text::new(format!("{shown} marqueurs ici")).color(TextColor::Gray),
        Text::new(""),
        arrow("[ ^ ]", Direction::North, "Vers le nord"),
        arrow("[ < ]", Direction::West, "Vers l'ouest")
            .append(Text::new("   "))
            .append(arrow("[ > ]", Direction::East, "Vers l'est")),
        arrow("[ v ]", Direction::South, "Vers le sud"),
        Text::new(""),
    ];
    lines.extend(details(view.selected.and_then(|i| markers.get(i))));
    lines
}

/// Samples the terrain and draws the whole dialog.
pub fn dialog(
    art: &MapArt,
    view: &View,
    markers: &[Marker],
    generator: &dyn WorldGenerator,
) -> Dialog {
    let terrain = Terrain::sample(generator, &view.window);
    let image = render(&terrain, &art.style);
    let go = |col: u32, row: u32| {
        let (col, row) = (u16::try_from(col).ok()?, u16::try_from(row).ok()?);
        Some(Action::Go(col, row).id())
    };
    let map = art.pixmap.paint(&image, ZONE, go, &stamps(view, markers));
    let center_height = terrain.height(COLS.get() / 2, ROWS.get() / 2);
    let block = art.pixmap.beside(
        map,
        u32::from(COLS.get()),
        GAP,
        PANEL,
        panel(view, markers, center_height),
    );
    Dialog::new(
        Text::new("Carte du monde"),
        Button::new("Retour au centre")
            .width(BUTTON_WIDTH)
            .on_click(Action::Home.id()),
    )
    .body(block, BODY_WIDTH)
    .exit(
        Button::new("Fermer")
            .width(BUTTON_WIDTH)
            .on_click(Action::Close.id()),
    )
}

#[cfg(test)]
mod tests {
    use voidmc::{BiomeId, DefaultWorldGenerator};
    use voidmc_codec::Encode;

    use super::*;
    use crate::markers::MarkerField;
    use crate::plugin::MARKERS;
    use crate::testing::Flat;

    fn marker(x: i32, z: i32) -> Marker {
        Marker {
            name: "Val-Clair".into(),
            kind: MarkerKind::Village,
            x,
            z,
            height: 70,
        }
    }

    #[test]
    fn every_action_survives_its_id() {
        let actions = [
            Action::Pan(Direction::North),
            Action::Pan(Direction::South),
            Action::Pan(Direction::East),
            Action::Pan(Direction::West),
            Action::Go(95, 71),
            Action::Select(23),
            Action::Home,
            Action::Close,
        ];
        for action in actions {
            assert_eq!(Action::parse(&action.id()), Some(action));
        }
    }

    #[test]
    fn foreign_and_malformed_ids_are_not_actions() {
        for id in [
            "voidmc:checkers/new",
            "voidmc:map",
            "voidmc:map/go/1",
            "voidmc:map/go/-1/2",
            "voidmc:map/go/70000/2",
            "voidmc:map/marker/x",
            "voidmc:map/pan/up",
        ] {
            assert_eq!(Action::parse(id), None, "{id}");
        }
    }

    #[test]
    fn clicks_move_the_view_and_home_resets_it() {
        let mut view = View::default();
        view.apply(Action::Pan(Direction::East), &[]);
        assert_eq!(view.window().center(), (96, 0));
        view.apply(Action::Go(48, 0), &[]);
        assert_eq!(view.window().center(), (96, -144));
        view.apply(Action::Home, &[]);
        assert_eq!(view, View::default());
    }

    #[test]
    fn forged_clicks_change_nothing() {
        let mut view = View::default();
        view.apply(Action::Go(COLS.get(), 0), &[]);
        view.apply(Action::Select(3), &[marker(0, 0)]);
        assert_eq!(view, View::default());
    }

    #[test]
    fn a_selected_marker_turns_white_and_fills_the_panel() {
        let markers = [marker(0, 0)];
        let mut view = View::default();
        let before = stamps(&view, &markers);
        view.apply(Action::Select(0), &markers);
        assert_ne!(before, stamps(&view, &markers));
        let lines = panel(&view, &markers, 70);
        assert!(lines.contains(&Text::new("Val-Clair").color(TextColor::Gold)));
    }

    #[test]
    fn the_panel_fits_beside_the_map() {
        let art = MapArt::new(&mut ResourcePack::new("test"));
        let markers = [marker(0, 0)];
        let mut view = View::default();
        view.apply(Action::Select(0), &markers);
        assert!(panel(&view, &markers, 70).len() < art.pixmap.lines());
    }

    #[test]
    fn the_generator_draws_the_map() {
        let art = MapArt::new(&mut ResourcePack::new("test"));
        let view = View::default();
        let land = Flat::new(70, BiomeId::plains());
        let water = Flat::new(40, BiomeId::plains());
        assert_ne!(
            dialog(&art, &view, &[], &land),
            dialog(&art, &view, &[], &water)
        );
    }

    /// The client drops a frame over 2 MiB, and this server does not compress.
    #[test]
    fn the_packet_stays_far_below_the_frame_limit() {
        const BUDGET: usize = 512 * 1024;
        let generator = DefaultWorldGenerator::default();
        let art = MapArt::new(&mut ResourcePack::new("test"));
        let markers = MarkerField { ..MARKERS }.place(&generator);
        for (x, z) in [(0, 0), (160, 160), (-300, 80), (5000, -5000)] {
            let mut view = View::default();
            view.window.center_on(x, z);
            let mut bytes = Vec::new();
            dialog(&art, &view, &markers, &generator)
                .packet()
                .encode(&mut bytes);
            println!("packet at ({x}, {z}): {} bytes", bytes.len());
            assert!(bytes.len() < BUDGET, "{} bytes", bytes.len());
        }
    }
}
