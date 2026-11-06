//! The settings of the map, as a dialog that stays open over it: every choice on one
//! screen, and a click applies it at once.
//!
//! [`settings`] is the only list of what can be changed, and [`PREVIEWS`] of what can be
//! watched. The dialog, its clicks and the name that comes up all follow from those.

use voidmc::{RegistryDataStore, TextColor};
use voidmc_protocol::clientbound::{RegistryData, RegistryEntry};
use voidmc_ui::{Button, Dialog, Image, ResourcePack, Text};

use crate::art::Style;
use crate::fence::Border;
use crate::goto::Navigation;
use crate::life::show::LifeLook;
use crate::players::BoatLook;
use crate::plugin::{MapLooks, Me};
use crate::regions::ZoneLook;
use crate::scene::Blocks;
use crate::transition::Flow;
use crate::weather::Weather;

const NAMESPACE: &str = "voidmap:settings";
/// The boss bar sits right where a title goes, and the buttons say what this is.
const TITLE: &str = " ";
const MENU_BUTTON: &str = "Réglages carte";
const DIALOGS: &str = "minecraft:dialog";
/// The dialogs of the client's menu, and this one in it.
const MENU_TAG: &str = "minecraft:pause_screen_additions";
const MENU_DIALOG: &str = "voidmap:settings";
const CLOSE: &str = "Fermer";
const PREVIEW_ROW: &str = "Aperçus";
/// Choices side by side, next to the name of the setting.
const CHOICES_PER_ROW: usize = 4;
const NAME_WIDTH: i32 = 70;
const CHOICE_WIDTH: i32 = 100;
const CLOSE_WIDTH: i32 = 120;
/// A cell with nothing in it. The client lays out a last row of its own if it falls short.
/// So thin that only the transparent edge of [`button`] shows of it.
const FILLER_WIDTH: i32 = 1;

/// What the client draws behind a screen. No blur, or the map would be a smear, and hardly
/// any shade over it. It goes for every screen of the client while the pack is loaded.
const BLUR_FILE: &str = "assets/minecraft/post_effect/blur.json";
const NO_BLUR: &str = r#"{"targets":{},"passes":[]}"#;
const SHADE_FILE: &str = "assets/minecraft/textures/gui/inworld_menu_background.png";
const SHADE: [u8; 4] = [0, 0, 0, 48];
const BUTTON_FILE: &str = "assets/minecraft/textures/gui/sprites/widget/button";
const HOVERED_BUTTON_FILE: &str = "assets/minecraft/textures/gui/sprites/widget/button_highlighted";
const BUTTON_SIZE: (u32, u32) = (200, 20);
const BUTTON_SLICES: &str =
    r#"{"gui":{"scaling":{"type":"nine_slice","width":200,"height":20,"border":3}}}"#;
const BUTTON_FILL: [u8; 4] = [12, 18, 30, 205];
const BUTTON_EDGE: [u8; 4] = [64, 104, 150, 255];
const HOVERED_FILL: [u8; 4] = [28, 42, 66, 235];
const HOVERED_EDGE: [u8; 4] = [255, 196, 48, 255];

type Reader = Box<dyn Fn(MapLooks) -> usize>;
type Writer = Box<dyn Fn(&mut MapLooks, usize)>;

/// Something a player can set, and how it reads and writes [`MapLooks`].
pub struct Setting {
    name: &'static str,
    labels: Vec<&'static str>,
    current: Reader,
    choose: Writer,
}

impl Setting {
    /// The choices of a field of the looks, with what each is called.
    fn new<T: Copy + PartialEq + 'static>(
        name: &'static str,
        all: &'static [T],
        label: fn(T) -> &'static str,
        field: fn(&mut MapLooks) -> &mut T,
    ) -> Self {
        Self {
            name,
            labels: all.iter().map(|one| label(*one)).collect(),
            current: Box::new(move |mut looks| {
                let now = *field(&mut looks);
                all.iter().position(|one| *one == now).unwrap_or(0)
            }),
            choose: Box::new(move |looks, index| {
                if let Some(one) = all.get(index) {
                    *field(looks) = *one;
                }
            }),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn label(&self, choice: usize) -> &'static str {
        self.labels.get(choice).copied().unwrap_or_default()
    }

    pub fn choose(&self, looks: &mut MapLooks, choice: usize) {
        (self.choose)(looks, choice);
    }
}

/// All that can be changed of the map, top to bottom. A new one goes here, as a line, and
/// [`MapLooks`] holds its value: it reaches a player's map from there.
pub fn settings() -> Vec<Setting> {
    vec![
        Setting::new("Panneau", &Style::ALL, Style::label, |looks| {
            &mut looks.style
        }),
        Setting::new("Marqueur", &Me::ALL, Me::label, |looks| &mut looks.me),
        Setting::new("Zones", &ZoneLook::ALL, ZoneLook::label, |looks| {
            &mut looks.zone
        }),
        Setting::new("Blocs", &Blocks::ALL, Blocks::label, |looks| {
            &mut looks.blocks
        }),
        Setting::new("Frontières", &Border::ALL, Border::label, |looks| {
            &mut looks.border
        }),
        Setting::new("Météo", &Weather::ALL, Weather::label, |looks| {
            &mut looks.weather
        }),
        Setting::new("Animations", &LifeLook::ALL, LifeLook::label, |looks| {
            &mut looks.life
        }),
        Setting::new("Bateaux", &BoatLook::ALL, BoatLook::label, |looks| {
            &mut looks.boats
        }),
        Setting::new(
            "Déplacement",
            &Navigation::ALL,
            Navigation::label,
            |looks| &mut looks.navigation,
        ),
        Setting::new("Intro", &Flow::ALL, Flow::label, |looks| &mut looks.intro),
        Setting::new("Outro", &Flow::ALL, Flow::label, |looks| &mut looks.outro),
    ]
}

/// An animation to watch without leaving the map. `plugin` plays them, in `play_preview`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preview {
    Intro,
    Outro,
    Teleport,
}

/// What the dialog offers to watch. A new one goes here, as a line, and in `play_preview`.
pub const PREVIEWS: [(&str, Preview); 3] = [
    ("Voir l'intro", Preview::Intro),
    ("Voir l'outro", Preview::Outro),
    ("Téléportation", Preview::Teleport),
];

/// What a player can ask for by clicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A choice of a setting, both by their place in their list.
    Set(usize, usize),
    Preview(usize),
    Close,
}

impl Action {
    pub(crate) fn id(self) -> String {
        match self {
            Self::Set(setting, choice) => format!("{NAMESPACE}/set/{setting}/{choice}"),
            Self::Preview(preview) => format!("{NAMESPACE}/preview/{preview}"),
            Self::Close => format!("{NAMESPACE}/close"),
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        let path = id.strip_prefix(NAMESPACE)?.strip_prefix('/')?;
        let mut parts = path.split('/');
        let action = match parts.next()? {
            "set" => Self::Set(parts.next()?.parse().ok()?, parts.next()?.parse().ok()?),
            "preview" => Self::Preview(parts.next()?.parse().ok()?),
            "close" => Self::Close,
            _ => return None,
        };
        parts.next().is_none().then_some(action)
    }
}

/// A cell that says what the row is about. A button with no action, as the client has no
/// other way to put a text in the grid.
fn name(text: &str) -> Button {
    Button::with_label(Text::new(text).color(TextColor::Aqua)).width(NAME_WIDTH)
}

fn filler() -> Button {
    Button::new("").width(FILLER_WIDTH)
}

fn choice(label: &str, on: bool, action: Action) -> Button {
    let text = if on {
        Text::new(format!("» {label}")).color(TextColor::Gold)
    } else {
        Text::new(label)
    };
    Button::with_label(text)
        .width(CHOICE_WIDTH)
        .on_click(action.id())
}

/// Rows of a name and choices, each row as wide as the others.
fn row_of(title: &str, choices: Vec<Button>) -> Vec<Button> {
    let mut cells = Vec::new();
    for (index, row) in choices.chunks(CHOICES_PER_ROW).enumerate() {
        cells.push(if index == 0 { name(title) } else { filler() });
        cells.extend_from_slice(row);
        cells.extend(std::iter::repeat_n(filler(), CHOICES_PER_ROW - row.len()));
    }
    cells
}

/// All the settings and what each is on, for the looks of a player.
pub fn dialog(looks: MapLooks) -> Dialog {
    grid(Some(looks))
}

/// A button in the menu of the client, the only one a pointer can reach from the map. The
/// client shows this as it is, without asking: it knows nothing of what is on, and the
/// first click brings [`dialog`] instead.
pub fn register(store: &mut RegistryDataStore) {
    let dialog = grid(None).external_title(Text::new(MENU_BUTTON));
    store.add_registry(RegistryData {
        registry_id: DIALOGS.to_string(),
        entries: vec![RegistryEntry {
            entry_id: MENU_DIALOG.to_string(),
            data: Some(dialog.to_nbt()),
        }],
    });
    store.add_tag(DIALOGS, MENU_TAG, &[MENU_DIALOG]);
}

fn grid(looks: Option<MapLooks>) -> Dialog {
    let settings = settings()
        .into_iter()
        .enumerate()
        .flat_map(|(at, setting)| {
            let now = looks.map(|looks| (setting.current)(looks));
            let choices = setting.labels.iter().enumerate();
            let choices = choices
                .map(|(index, label)| choice(label, Some(index) == now, Action::Set(at, index)));
            row_of(setting.name, choices.collect())
        });
    let previews = PREVIEWS.iter().enumerate();
    let previews = previews.map(|(at, (label, _))| choice(label, false, Action::Preview(at)));
    let cells = settings.chain(row_of(PREVIEW_ROW, previews.collect()));
    let close = Button::new(CLOSE)
        .width(CLOSE_WIDTH)
        .on_click(Action::Close.id());
    let mut cells = cells.collect::<Vec<_>>().into_iter();
    let first = cells.next().unwrap_or_else(filler);
    cells
        .fold(Dialog::new(Text::new(TITLE), first), Dialog::button)
        .columns(CHOICES_PER_ROW as i32 + 1)
        .exit(close)
}

/// A plain button that lets the map through, with its outermost pixels left out.
fn button(fill: [u8; 4], edge: [u8; 4]) -> Image {
    let (width, height) = BUTTON_SIZE;
    let mut image = Image::new(width, height);
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let outline = x == 1 || y == 1 || x == width - 2 || y == height - 2;
            image.set(x, y, if outline { edge } else { fill });
        }
    }
    image
}

/// Leaves the map to see behind the dialog. A client without the pack blurs it.
pub fn install(pack: &mut ResourcePack) {
    pack.add(BLUR_FILE, NO_BLUR.as_bytes().to_vec());
    pack.add(SHADE_FILE, Image::filled(16, 16, SHADE).to_png());
    for (file, sprite) in [
        (BUTTON_FILE, button(BUTTON_FILL, BUTTON_EDGE)),
        (HOVERED_BUTTON_FILE, button(HOVERED_FILL, HOVERED_EDGE)),
    ] {
        pack.add(format!("{file}.png"), sprite.to_png());
        pack.add(
            format!("{file}.png.mcmeta"),
            BUTTON_SLICES.as_bytes().to_vec(),
        );
    }
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::{List, Tag};

    use super::*;

    fn buttons(looks: MapLooks) -> Vec<ussr_nbt::owned::Compound> {
        let nbt = dialog(looks).to_nbt();
        let actions = nbt
            .compound
            .tags
            .iter()
            .find(|(name, _)| name.to_string() == "actions");
        match actions {
            Some((_, Tag::List(List::Compound(buttons)))) => buttons.clone(),
            _ => panic!("no buttons"),
        }
    }

    #[test]
    fn an_action_survives_its_id() {
        for action in [Action::Set(3, 7), Action::Preview(1), Action::Close] {
            assert_eq!(Action::parse(&action.id()), Some(action));
        }
    }

    #[test]
    fn other_ids_are_left_to_others() {
        for id in [
            "voidmc:checkers/new",
            "voidmap:settings/set/1",
            "voidmap:settings/close/1",
        ] {
            assert_eq!(Action::parse(id), None);
        }
    }

    /// The client lays out a short last row on its own, out of line with the others.
    #[test]
    fn every_row_is_full() {
        let columns = CHOICES_PER_ROW + 1;
        assert_eq!(buttons(MapLooks::default()).len() % columns, 0);
    }

    #[test]
    fn a_choice_of_each_setting_comes_back_as_the_current_one() {
        for setting in settings() {
            for index in 0..setting.labels.len() {
                let mut looks = MapLooks::default();
                setting.choose(&mut looks, index);
                assert_eq!((setting.current)(looks), index, "{}", setting.name);
            }
        }
    }

    #[test]
    fn a_click_past_the_end_changes_nothing() {
        let mut looks = MapLooks::default();
        settings()[0].choose(&mut looks, 99);
        assert_eq!(looks, MapLooks::default());
    }
}
