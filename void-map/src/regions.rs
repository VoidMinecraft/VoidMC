//! What a player knows of the map, region by region. The model only shows a state:
//! it never decides one.

use voidmc_data::v26_1_2::blocks as b;

use crate::noise::fractal;

/// How far region borders stray from straight lines.
const BORDER_WANDER: f64 = 70.0;
const BORDER_SIZE: f64 = 180.0;
const WANDER_SEED: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionState {
    Discovered,
    Undiscovered,
    Locked,
}

impl RegionState {
    /// Name shown to players.
    pub fn label(self) -> &'static str {
        match self {
            Self::Discovered => "Decouverte",
            Self::Undiscovered => "Non decouverte",
            Self::Locked => "Verrouillee",
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

#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub name: &'static str,
    /// Its centre, in world blocks.
    pub site: [f64; 2],
    pub state: RegionState,
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
        let region = |name, site, state| Region { name, site, state };
        Self::new(vec![
            region("Terres du Centre", [0.0, 60.0], RegionState::Discovered),
            region("Cote Ouest", [-300.0, 40.0], RegionState::Discovered),
            region("Cap Sud", [80.0, 340.0], RegionState::Discovered),
            region("Marches du Nord", [40.0, -340.0], RegionState::Undiscovered),
            region("Confins de l'Est", [340.0, -60.0], RegionState::Locked),
        ])
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
        assert_eq!(regions.at(-300.0, 40.0).name, "Cote Ouest");
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
