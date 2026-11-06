//! What stands along the border of a closed region, on a band a few blocks wide.
//!
//! The camera looks almost straight down, so what tells a design is the top of it, and a
//! coarser model has fewer cells for that top. Each design is drawn for the width of its
//! band, in cells, rather than shrunk from the close model.

use voidmc_data::v26_1_2::blocks as b;

/// How high a wall stands over the land, in blocks of a landmark.
const WALL_HEIGHT: i32 = 2;
/// How high a tower stands over its wall, in blocks of a landmark.
const TOWER_HEIGHT: i32 = 2;
/// How high a wall of glass stands over the land, in blocks of a landmark.
const GLASS_HEIGHT: i32 = 3;
/// How many blocks at its top make up its colour.
const GLASS_LAYERS: i32 = 3;

/// What a player sees along the border of a closed region, under [`ZoneLook::Palisade`].
///
/// [`ZoneLook::Palisade`]: crate::regions::ZoneLook::Palisade
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Border {
    /// A stone wall with a merlon every other block.
    #[default]
    Wall,
    /// The same wall, with a square tower here and there.
    Rampart,
    /// A wall of coloured glass, which lets the land show through.
    Glass,
}

impl Border {
    pub const ALL: [Self; 3] = [Self::Wall, Self::Rampart, Self::Glass];

    pub fn label(self) -> &'static str {
        match self {
            Self::Wall => "Muraille",
            Self::Rampart => "Rempart",
            Self::Glass => "Verre",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|one| *one == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// The blocks over a column of the band, as a height over its top and a block. `real`
    /// asks for the blocks of the world rather than flat colours.
    pub fn blocks(self, at: Stretch, real: bool) -> Vec<(i32, i32)> {
        match self {
            Self::Wall => wall(at, real),
            Self::Rampart if at.tower => tower(at, real),
            Self::Rampart => wall(at, real),
            Self::Glass => glass(at),
        }
    }
}

/// A column of the band along a border.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stretch {
    pub x: i32,
    pub z: i32,
    /// How far in from the border, from 1.
    pub ring: u8,
    /// How many rings the band has on this model.
    pub width: u8,
    /// Whether it stands in a tower of a rampart.
    pub tower: bool,
    /// How far the land around it stands over this column: what stands on a band keeps
    /// one level over a stretch of it, rather than the outline of every terrace.
    pub rise: i32,
    /// Model blocks per block of a landmark.
    pub build: i32,
    pub locked: bool,
}

impl Stretch {
    /// One column in two along a band, on every model and whichever way it runs: a band
    /// goes from one column to the next by a side, so each step changes this.
    fn is_every_other(self) -> bool {
        (self.x + self.z).rem_euclid(2) == 0
    }

    /// Whether a band has a parapet here: on its outer edge, and on the inner one if it has
    /// room for a walkway between them.
    fn is_parapet(self) -> bool {
        self.ring == 1 || (self.width > 2 && self.ring == self.width)
    }
}

fn pile(from: i32, to: i32, block: i32) -> impl Iterator<Item = (i32, i32)> {
    (from..=to).map(move |y| (y, block))
}

/// The stone of a wall and the stone of what stands on it.
fn stones(real: bool) -> (i32, i32) {
    if real {
        (b::STONE_BRICKS, b::SMOOTH_STONE)
    } else {
        (b::GRAY_CONCRETE, b::LIGHT_GRAY_CONCRETE)
    }
}

fn wall(at: Stretch, real: bool) -> Vec<(i32, i32)> {
    let (body, merlon) = stones(real);
    let walkway = at.rise + WALL_HEIGHT * at.build;
    let blocks = pile(1, walkway, body);
    if at.is_parapet() && at.is_every_other() {
        blocks
            .chain(pile(walkway + 1, walkway + at.build, merlon))
            .collect()
    } else {
        blocks.collect()
    }
}

fn tower(at: Stretch, real: bool) -> Vec<(i32, i32)> {
    let (body, _) = stones(real);
    let roof = if at.locked {
        b::RED_TERRACOTTA
    } else {
        b::CYAN_TERRACOTTA
    };
    let tall = at.rise + (WALL_HEIGHT + TOWER_HEIGHT) * at.build;
    pile(1, tall - 1, body).chain([(tall, roof)]).collect()
}

fn glass(at: Stretch) -> Vec<(i32, i32)> {
    let (pane, other) = if at.locked {
        (b::RED_STAINED_GLASS, b::ORANGE_STAINED_GLASS)
    } else {
        (b::LIGHT_GRAY_STAINED_GLASS, b::WHITE_STAINED_GLASS)
    };
    let tall = at.rise + GLASS_HEIGHT * at.build;
    // The faces between two panes of the same glass are not drawn, so a pile of it is as
    // thin as one pane. Its top alternates with another one for the colour to add up, or a
    // band of one cell would not show from afar.
    let other_at = |y: i32| y % 2 == 0 && y > tall - GLASS_LAYERS;
    (1..=tall)
        .map(|y| (y, if other_at(y) { other } else { pane }))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn stretch(ring: u8) -> Stretch {
        Stretch {
            x: 4,
            z: 9,
            ring,
            width: 3,
            tower: false,
            rise: 0,
            build: 2,
            locked: true,
        }
    }

    fn top(blocks: &[(i32, i32)]) -> i32 {
        blocks.iter().map(|(y, _)| *y).max().unwrap()
    }

    #[test]
    fn every_design_is_listed_and_the_list_goes_round() {
        let mut border = Border::default();
        for _ in Border::ALL {
            border = border.next();
        }
        assert_eq!(border, Border::default());
    }

    #[test]
    fn a_wall_is_the_default() {
        assert_eq!(Border::default(), Border::Wall);
    }

    #[test]
    fn designs_really_differ() {
        let tower = Stretch {
            tower: true,
            ..stretch(2)
        };
        let looks: Vec<_> = Border::ALL
            .iter()
            .map(|border| border.blocks(tower, true))
            .collect();
        for (index, look) in looks.iter().enumerate() {
            assert!(!look.is_empty());
            assert!(!looks[..index].contains(look));
        }
    }

    /// Whatever way a wall runs, the next column along it is a gap if this one is a merlon.
    #[test]
    fn a_wall_has_a_merlon_every_other_block_on_every_model() {
        for (width, build) in [(3, 2), (2, 1), (1, 1)] {
            let at = |x, z| {
                let column = Stretch {
                    x,
                    z,
                    width,
                    build,
                    ring: 1,
                    ..stretch(1)
                };
                top(&Border::Wall.blocks(column, false))
            };
            for (dx, dz) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                assert_eq!(at(4, 8), at(4 + 2 * dx, 8 + 2 * dz));
                assert_eq!(at(4, 8), at(4 + dx, 8 + dz) + build);
            }
        }
    }

    #[test]
    fn a_walkway_runs_between_two_parapets_where_there_is_room() {
        let (x, z) = (4, 8);
        let at = |ring| Stretch {
            x,
            z,
            ..stretch(ring)
        };
        let tops = [1, 2, 3].map(|ring| top(&Border::Wall.blocks(at(ring), false)));
        assert!(tops[0] > tops[1] && tops[2] > tops[1], "{tops:?}");
    }

    #[test]
    fn what_stands_on_a_band_keeps_one_level_over_the_land() {
        for border in Border::ALL {
            let low = Stretch {
                rise: 3,
                ..stretch(2)
            };
            assert_eq!(
                top(&border.blocks(low, false)),
                top(&border.blocks(stretch(2), false)) + 3,
                "{border:?}"
            );
        }
    }

    #[test]
    fn a_tower_rises_over_its_wall_and_only_on_a_rampart() {
        let tower = Stretch {
            tower: true,
            ..stretch(1)
        };
        let wall = top(&Border::Rampart.blocks(stretch(2), true));
        assert!(top(&Border::Rampart.blocks(tower, true)) > wall);
        assert_eq!(
            top(&Border::Wall.blocks(tower, true)),
            top(&Border::Wall.blocks(stretch(1), true))
        );
    }

    /// A band of one cell has to show as well as one of three.
    #[test]
    fn glass_mixes_two_panes_on_every_model() {
        for (width, build) in [(3, 2), (2, 1), (1, 1)] {
            let at = Stretch {
                width,
                build,
                ..stretch(1)
            };
            let blocks = Border::Glass.blocks(at, false);
            let panes: BTreeSet<_> = blocks.iter().map(|(_, block)| *block).collect();
            assert_eq!(panes.len(), 2, "{width}");
        }
    }
}
