//! What a player knows of the map, region by region. The model only shows a state:
//! it never decides one.

use voidmc_data::v26_1_2::blocks as b;

use crate::noise::fractal;

/// How far region borders stray from straight lines.
const BORDER_WANDER: f64 = 70.0;
const BORDER_SIZE: f64 = 180.0;
const WANDER_SEED: u32 = 50;

/// From the most open to the most closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegionState {
    Discovered,
    Undiscovered,
    Locked,
}

impl RegionState {
    /// Name shown to players.
    pub fn label(self) -> &'static str {
        match self {
            Self::Discovered => "Découverte",
            Self::Undiscovered => "Non découverte",
            Self::Locked => "Verrouillée",
        }
    }

    /// The block standing for `block` in a region in this state. A place stays
    /// recognisable under its state: this only takes the colour down a little.
    pub fn tint(self, block: i32) -> i32 {
        match self {
            Self::Discovered => block,
            Self::Undiscovered => muted(block),
            Self::Locked => dimmed(muted(block)),
        }
    }
}

/// The same colour with less of it.
fn muted(block: i32) -> i32 {
    match block {
        b::LIME_CONCRETE => b::LIME_TERRACOTTA,
        b::GREEN_CONCRETE | b::LIME_TERRACOTTA => b::GREEN_TERRACOTTA,
        b::LIGHT_BLUE_CONCRETE => b::CYAN_CONCRETE,
        b::SMOOTH_SANDSTONE => b::WHITE_TERRACOTTA,
        other => other,
    }
}

/// The grey standing for a block, whatever its colour.
pub fn faded(block: i32) -> i32 {
    match block {
        b::LIGHT_BLUE_CONCRETE | b::CYAN_CONCRETE => b::GRAY_CONCRETE,
        b::WHITE_CONCRETE | b::SMOOTH_SANDSTONE => b::WHITE_CONCRETE,
        b::BLACK_CONCRETE | b::GRAY_CONCRETE => block,
        _ => b::LIGHT_GRAY_CONCRETE,
    }
}

/// A shade under a muted colour.
fn dimmed(block: i32) -> i32 {
    match block {
        b::LIME_TERRACOTTA => b::GREEN_TERRACOTTA,
        b::WHITE_TERRACOTTA => b::LIGHT_GRAY_TERRACOTTA,
        b::WHITE_CONCRETE => b::LIGHT_GRAY_CONCRETE,
        b::LIGHT_GRAY_CONCRETE => b::GRAY_CONCRETE,
        other => other,
    }
}

/// How the model shows a region that is not open yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ZoneLook {
    /// Clouds of blocks hang over it.
    Clouds,
    /// Diagonal stripes on the ground.
    Hatching,
    /// A [`Border`](crate::fence::Border) along its edge, with a lock over it.
    #[default]
    Palisade,
    /// Its colours drained, with a large question mark over it.
    Faded,
}

impl ZoneLook {
    pub const ALL: [Self; 4] = [Self::Clouds, Self::Hatching, Self::Palisade, Self::Faded];

    pub fn label(self) -> &'static str {
        match self {
            Self::Clouds => "Nuages",
            Self::Hatching => "Hachures",
            Self::Palisade => "Palissade",
            Self::Faded => "Couleurs éteintes",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Clouds => Self::Hatching,
            Self::Hatching => Self::Palisade,
            Self::Palisade => Self::Faded,
            Self::Faded => Self::Clouds,
        }
    }
}

/// What falls on a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Climate {
    #[default]
    Clear,
    Rain,
    Snow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub name: &'static str,
    /// Its centre, in world blocks.
    pub site: [f64; 2],
    pub state: RegionState,
    pub climate: Climate,
}

/// Regions cover the whole map: each place belongs to the nearest site.
#[derive(Debug, Clone, PartialEq)]
pub struct Regions(Vec<Region>);

impl Regions {
    pub fn new(regions: Vec<Region>) -> Self {
        assert!(!regions.is_empty(), "a map has at least one region");
        Self(regions)
    }

    /// A fixed map with one region of each kind left to open up.
    pub fn demo() -> Self {
        let region = |name, site, state, climate| Region {
            name,
            site,
            state,
            climate,
        };
        let (open, unknown) = (RegionState::Discovered, RegionState::Undiscovered);
        Self::new(vec![
            region("Terres du Centre", [0.0, 60.0], open, Climate::Clear),
            region("Côte Ouest", [-300.0, 40.0], open, Climate::Rain),
            region("Cap Sud", [80.0, 340.0], open, Climate::Clear),
            region("Marches du Nord", [40.0, -340.0], unknown, Climate::Snow),
            region(
                "Confins de l'Est",
                [340.0, -60.0],
                RegionState::Locked,
                Climate::Rain,
            ),
        ])
    }

    /// Those that are not open yet.
    pub fn closed(&self) -> impl Iterator<Item = &Region> {
        self.0
            .iter()
            .filter(|region| region.state != RegionState::Discovered)
    }

    pub fn at(&self, x: f64, z: f64) -> &Region {
        let wander = |seed| fractal(WANDER_SEED + seed, x, z, BORDER_SIZE) * BORDER_WANDER;
        let (x, z) = (x + wander(0), z + wander(10));
        let distance = |region: &Region| (region.site[0] - x).hypot(region.site[1] - z);
        self.0
            .iter()
            .min_by(|a, b| distance(a).total_cmp(&distance(b)))
            .unwrap_or(&self.0[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_belongs_to_its_own_region() {
        let regions = Regions::demo();
        assert_eq!(regions.at(-300.0, 40.0).name, "Côte Ouest");
        assert_eq!(regions.at(340.0, -60.0).state, RegionState::Locked);
    }

    #[test]
    fn a_discovered_region_shows_its_real_colours() {
        assert_eq!(
            RegionState::Discovered.tint(b::LIME_CONCRETE),
            b::LIME_CONCRETE
        );
    }

    #[test]
    fn fog_keeps_grass_green_and_the_sea_blue() {
        let undiscovered = RegionState::Undiscovered;
        assert_eq!(undiscovered.tint(b::LIME_CONCRETE), b::LIME_TERRACOTTA);
        assert_eq!(undiscovered.tint(b::LIGHT_BLUE_CONCRETE), b::CYAN_CONCRETE);
    }

    #[test]
    fn a_locked_region_is_a_shade_under_an_undiscovered_one() {
        for block in [b::LIME_CONCRETE, b::SMOOTH_SANDSTONE, b::WHITE_CONCRETE] {
            assert_ne!(
                RegionState::Locked.tint(block),
                RegionState::Undiscovered.tint(block)
            );
        }
    }

    #[test]
    fn every_place_has_a_region() {
        let regions = Regions::demo();
        for x in (-464..464).step_by(29) {
            for z in (-464..464).step_by(29) {
                assert!(!regions.at(f64::from(x), f64::from(z)).name.is_empty());
            }
        }
    }
}
