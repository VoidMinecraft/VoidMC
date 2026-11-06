//! Reproducible points of interest scattered over the land.

use voidmc::WorldGenerator;

use crate::window::Window;

/// Tries allowed per wanted marker before settling for fewer.
const ATTEMPTS_PER_MARKER: usize = 64;

const NAME_STARTS: [&str; 6] = ["Port", "Val", "Mont", "Roche", "Fort", "Bourg"];
const NAME_ENDS: [&str; 6] = ["Brume", "Clair", "Noir", "Givre", "Cendre", "Sombre"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    Village,
    Ruins,
    Mine,
    Lighthouse,
}

impl MarkerKind {
    pub const ALL: [Self; 4] = [Self::Village, Self::Ruins, Self::Mine, Self::Lighthouse];

    /// Name shown to players.
    pub fn label(self) -> &'static str {
        match self {
            Self::Village => "Village",
            Self::Ruins => "Ruines",
            Self::Mine => "Mine",
            Self::Lighthouse => "Phare",
        }
    }

    /// A line about what stands there.
    pub fn description(self) -> &'static str {
        match self {
            Self::Village => "Des maisons, un marché, du monde.",
            Self::Ruins => "Il ne reste que des pierres.",
            Self::Mine => "Une galerie encore étayée.",
            Self::Lighthouse => "Son feu se voit de loin.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub name: String,
    pub kind: MarkerKind,
    pub x: i32,
    pub z: i32,
    /// Surface height at the marker.
    pub height: i32,
}

/// Where markers may land. The same field and generator always give the same
/// markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerField {
    pub seed: u64,
    pub count: usize,
    /// Markers stay within this many blocks of the origin on each axis.
    pub radius: u32,
    /// Smallest distance between two markers, in blocks.
    pub min_spacing: u32,
    /// Markers only land above this height.
    pub sea_level: i32,
}

impl MarkerField {
    /// Returns at most `count` markers: fewer when the land is too scarce or
    /// the spacing too wide to fit them all.
    pub fn place(&self, generator: &dyn WorldGenerator) -> Vec<Marker> {
        let mut random = SplitMix64(self.seed);
        let names = shuffled_names(&mut random);
        let mut markers = Vec::with_capacity(self.count);
        for _ in 0..self.count.saturating_mul(ATTEMPTS_PER_MARKER) {
            if markers.len() == self.count {
                break;
            }
            let (x, z) = (
                random.coordinate(self.radius),
                random.coordinate(self.radius),
            );
            let height = generator.surface_height_at(x, z);
            if height <= self.sea_level || self.crowded(&markers, x, z) {
                continue;
            }
            markers.push(Marker {
                name: numbered(&names, markers.len()),
                kind: MarkerKind::ALL[random.below(MarkerKind::ALL.len() as u64) as usize],
                x,
                z,
                height,
            });
        }
        markers
    }

    fn crowded(&self, markers: &[Marker], x: i32, z: i32) -> bool {
        let spacing = i64::from(self.min_spacing).pow(2);
        markers.iter().any(|marker| {
            let (dx, dz) = (i64::from(marker.x - x), i64::from(marker.z - z));
            dx * dx + dz * dz < spacing
        })
    }
}

fn shuffled_names(random: &mut SplitMix64) -> Vec<String> {
    let mut names: Vec<String> = NAME_STARTS
        .iter()
        .flat_map(|start| NAME_ENDS.iter().map(move |end| format!("{start}-{end}")))
        .collect();
    for last in (1..names.len()).rev() {
        names.swap(last, random.below(last as u64 + 1) as usize);
    }
    names
}

/// Names stay unique past the end of the list: `Val-Clair 2`, and so on.
fn numbered(names: &[String], index: usize) -> String {
    let name = &names[index % names.len()];
    match index / names.len() {
        0 => name.clone(),
        round => format!("{name} {}", round + 1),
    }
}

/// Markers the view shows, as `(index in markers, col, row)`.
pub fn visible<'a>(
    markers: &'a [Marker],
    window: &'a Window,
) -> impl Iterator<Item = (usize, u16, u16)> + 'a {
    markers.iter().enumerate().filter_map(|(index, marker)| {
        let (col, row) = window.cell_at(marker.x, marker.z)?;
        Some((index, col, row))
    })
}

/// Written out so that placement never changes with a dependency update.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^ (mixed >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn coordinate(&mut self, radius: u32) -> i32 {
        let radius = i64::from(radius.min(i32::MAX as u32));
        (self.below(radius as u64 * 2 + 1) as i64 - radius) as i32
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use voidmc::{BiomeId, DefaultWorldGenerator};

    use super::*;
    use crate::testing::{Flat, Shaped, window};

    const FIELD: MarkerField = MarkerField {
        seed: 7,
        count: 24,
        radius: 600,
        min_spacing: 48,
        sea_level: 62,
    };

    fn land() -> Flat {
        Flat::new(70, BiomeId::plains())
    }

    #[test]
    fn the_same_seed_gives_the_same_markers() {
        let generator = DefaultWorldGenerator::default();
        assert_eq!(FIELD.place(&generator), FIELD.place(&generator));
    }

    /// Guards the promise of reproducible maps across versions of this crate.
    #[test]
    fn placement_is_pinned_for_a_known_seed() {
        let first = FIELD.place(&land()).remove(0);
        assert_eq!(
            (first.name.as_str(), first.kind, first.x, first.z),
            ("Port-Noir", MarkerKind::Ruins, 234, 515)
        );
    }

    #[test]
    fn another_seed_gives_other_markers() {
        let other = MarkerField { seed: 8, ..FIELD };
        assert_ne!(FIELD.place(&land()), other.place(&land()));
    }

    #[test]
    fn the_wanted_count_fits_on_open_land() {
        assert_eq!(FIELD.place(&land()).len(), FIELD.count);
    }

    #[test]
    fn markers_stay_in_the_field_and_keep_their_distance() {
        let markers = FIELD.place(&DefaultWorldGenerator::default());
        assert!(!markers.is_empty());
        for (index, marker) in markers.iter().enumerate() {
            assert!(marker.x.abs() <= 600 && marker.z.abs() <= 600);
            for other in &markers[index + 1..] {
                let (dx, dz) = (f64::from(marker.x - other.x), f64::from(marker.z - other.z));
                assert!(dx.hypot(dz) >= 48.0);
            }
        }
    }

    #[test]
    fn markers_land_above_water_and_record_the_surface() {
        let generator = DefaultWorldGenerator::default();
        for marker in FIELD.place(&generator) {
            assert!(marker.height > FIELD.sea_level);
            assert_eq!(
                marker.height,
                generator.surface_height_at(marker.x, marker.z)
            );
        }
    }

    #[test]
    fn half_flooded_land_only_gets_markers_on_the_dry_half() {
        let generator = Shaped {
            height: |x: i32, _| if x < 0 { 40 } else { 70 },
            biome: |_, _| BiomeId::plains(),
        };
        let markers = FIELD.place(&generator);
        assert!(!markers.is_empty());
        assert!(markers.iter().all(|marker| marker.x >= 0));
    }

    #[test]
    fn a_world_of_water_gets_no_marker_and_placement_ends() {
        assert!(FIELD.place(&Flat::new(40, BiomeId::plains())).is_empty());
    }

    #[test]
    fn a_field_too_small_gets_fewer_markers() {
        let tight = MarkerField {
            radius: 10,
            ..FIELD
        };
        let placed = tight.place(&land()).len();
        assert!((1..FIELD.count).contains(&placed));
    }

    #[test]
    fn names_stay_unique_past_the_end_of_the_list() {
        let many = MarkerField {
            count: 80,
            radius: 5_000,
            ..FIELD
        };
        let markers = many.place(&land());
        assert_eq!(markers.len(), 80);
        let names: HashSet<_> = markers.iter().map(|marker| &marker.name).collect();
        assert_eq!(names.len(), 80);
    }

    #[test]
    fn a_zero_radius_field_holds_one_marker_at_the_origin() {
        let point = MarkerField { radius: 0, ..FIELD };
        let markers = point.place(&land());
        assert_eq!(markers.len(), 1);
        assert_eq!((markers[0].x, markers[0].z), (0, 0));
    }

    #[test]
    fn only_markers_inside_the_view_are_visible() {
        let marker = |x, z| Marker {
            name: String::new(),
            kind: MarkerKind::Mine,
            x,
            z,
            height: 70,
        };
        let markers = [marker(0, 0), marker(500, 0), marker(-8, 5)];
        let view = window(0, 0, 8, 6, 4);
        let shown: Vec<_> = visible(&markers, &view).collect();
        assert_eq!(shown, [(0, 4, 3), (2, 2, 4)]);
    }
}
