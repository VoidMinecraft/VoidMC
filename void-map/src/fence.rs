//! What stands along the border of a closed region, on a band a few blocks wide.

use voidmc_data::v26_1_2::blocks as b;

use crate::noise::scatter;

const FENCE_SEED: u32 = 200;
/// Cells of one colour along a band of [`Border::Tape`], in blocks of a landmark.
const TAPE_STRETCH: i32 = 3;
/// Cells between two merlons of a rampart, in blocks of a landmark.
const MERLON_STEP: i32 = 2;

/// What a player sees along the border of a closed region, under [`ZoneLook::Palisade`].
///
/// [`ZoneLook::Palisade`]: crate::regions::ZoneLook::Palisade
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Border {
    /// Rough logs of uneven heights, taller down the middle.
    #[default]
    Stakes,
    /// A stone wall with merlons on both sides and a tower here and there.
    Rampart,
    /// A low round hedge, with a few flowers on it.
    Hedge,
    /// Nothing that stands: a striped band painted on the land and the sea.
    Tape,
    /// A wall of coloured glass, which lets the land show through.
    Glass,
}

impl Border {
    pub const ALL: [Self; 5] = [
        Self::Stakes,
        Self::Rampart,
        Self::Hedge,
        Self::Tape,
        Self::Glass,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Stakes => "Pieux",
            Self::Rampart => "Rempart",
            Self::Hedge => "Haie",
            Self::Tape => "Bande peinte",
            Self::Glass => "Verre",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|one| *one == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// The blocks over a column of the band, as a height over its top and a block. A
    /// height of 0 takes the place of the top. `real` asks for the blocks of the world
    /// rather than flat colours.
    pub fn blocks(self, at: Stretch, real: bool) -> Vec<(i32, i32)> {
        match self {
            Self::Stakes => stakes(at, real),
            Self::Rampart => rampart(at, real),
            Self::Hedge => hedge(at, real),
            Self::Tape => tape(at),
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
    /// Model blocks per block of a landmark.
    pub build: i32,
    pub locked: bool,
}

impl Stretch {
    fn is_middle(self) -> bool {
        self.ring == self.width.div_ceil(2)
    }

    /// A number between 0 and 1, the same every time for this column.
    fn roll(self) -> f64 {
        scatter(FENCE_SEED, self.x, self.z)[0]
    }
}

fn pile(from: i32, to: i32, block: i32) -> impl Iterator<Item = (i32, i32)> {
    (from..=to).map(move |y| (y, block))
}

fn stakes(at: Stretch, real: bool) -> Vec<(i32, i32)> {
    let (body, tip) = if real {
        (b::SPRUCE_LOG, b::STRIPPED_SPRUCE_LOG)
    } else {
        (b::BROWN_CONCRETE, b::BROWN_TERRACOTTA)
    };
    let base = if at.is_middle() { 3 } else { 2 } * at.build;
    let tall = base + (at.roll() * f64::from(at.build + 1)) as i32;
    pile(1, tall - 1, body).chain([(tall, tip)]).collect()
}

fn rampart(at: Stretch, real: bool) -> Vec<(i32, i32)> {
    let (wall, walk) = if real {
        (b::STONE_BRICKS, b::SMOOTH_STONE)
    } else {
        (b::GRAY_CONCRETE, b::LIGHT_GRAY_CONCRETE)
    };
    if at.tower {
        let roof = if at.locked {
            b::RED_TERRACOTTA
        } else {
            b::CYAN_TERRACOTTA
        };
        let tall = 5 * at.build;
        return pile(1, tall - 1, wall).chain([(tall, roof)]).collect();
    }
    let walkway = 2 * at.build;
    let step = MERLON_STEP * at.build.max(1);
    let merlon = (at.x + at.z).rem_euclid(2 * step) < step;
    if at.is_middle() && at.width > 1 {
        pile(1, walkway - 1, wall)
            .chain([(walkway, walk)])
            .collect()
    } else if merlon {
        pile(1, walkway + at.build, wall).collect()
    } else {
        pile(1, walkway, wall).collect()
    }
}

fn hedge(at: Stretch, real: bool) -> Vec<(i32, i32)> {
    let leaves = if real {
        b::MOSS_BLOCK
    } else {
        b::GREEN_CONCRETE
    };
    let flower = if at.locked {
        b::RED_CONCRETE
    } else {
        b::PINK_CONCRETE
    };
    let base = if at.is_middle() { 2 } else { 1 } * at.build;
    let tall = base + i32::from(at.roll() > 0.6);
    let top = if at.roll() > 0.93 { flower } else { leaves };
    pile(1, tall - 1, leaves).chain([(tall, top)]).collect()
}

fn tape(at: Stretch) -> Vec<(i32, i32)> {
    let stretch = TAPE_STRETCH * at.build.max(1);
    let first = (at.x.div_euclid(stretch) + at.z.div_euclid(stretch)).rem_euclid(2) == 0;
    let block = match (first, at.locked) {
        (true, true) => b::RED_CONCRETE,
        (true, false) => b::GRAY_CONCRETE,
        (false, _) => b::WHITE_CONCRETE,
    };
    vec![(0, block)]
}

fn glass(at: Stretch) -> Vec<(i32, i32)> {
    let pane = if at.locked {
        b::RED_STAINED_GLASS
    } else {
        b::LIGHT_GRAY_STAINED_GLASS
    };
    let tall = i32::from(at.width + 1 - at.ring) * 2 * at.build;
    pile(1, tall, pane).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stretch(ring: u8) -> Stretch {
        Stretch {
            x: 4,
            z: 9,
            ring,
            width: 3,
            tower: false,
            build: 2,
            locked: true,
        }
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
    fn designs_really_differ() {
        let looks: Vec<_> = Border::ALL
            .iter()
            .map(|border| border.blocks(stretch(2), true))
            .collect();
        for (index, look) in looks.iter().enumerate() {
            assert!(!look.is_empty());
            assert!(!looks[..index].contains(look));
        }
    }

    #[test]
    fn a_painted_band_stands_on_nothing() {
        assert_eq!(Border::Tape.blocks(stretch(1), false).len(), 1);
        assert_eq!(Border::Tape.blocks(stretch(1), false)[0].0, 0);
    }

    #[test]
    fn stakes_are_tallest_down_the_middle() {
        let top = |ring| {
            let blocks = Border::Stakes.blocks(stretch(ring), true);
            blocks.iter().map(|(y, _)| *y).max().unwrap()
        };
        assert!(top(2) >= 6 && top(1) < 6 + 3);
    }

    #[test]
    fn a_tower_rises_over_its_wall() {
        let wall = Border::Rampart.blocks(stretch(1), true).len();
        let tower = Stretch {
            tower: true,
            ..stretch(1)
        };
        assert!(Border::Rampart.blocks(tower, true).len() > wall);
    }
}
