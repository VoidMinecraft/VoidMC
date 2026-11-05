//! Small landmarks standing on the model, one shape per kind of marker.

use voidmc_data::v26_1_2::blocks as b;

use crate::markers::MarkerKind;

/// A block of a landmark: `x`, height above the ground, `z`, block.
pub type Voxel = (i32, i32, i32, i32);

fn house(x: i32, z: i32) -> [Voxel; 8] {
    [
        (x, 0, z, b::WHITE_CONCRETE),
        (x + 1, 0, z, b::WHITE_CONCRETE),
        (x, 0, z + 1, b::WHITE_CONCRETE),
        (x + 1, 0, z + 1, b::WHITE_CONCRETE),
        (x, 1, z, b::RED_CONCRETE),
        (x + 1, 1, z, b::RED_CONCRETE),
        (x, 1, z + 1, b::RED_CONCRETE),
        (x + 1, 1, z + 1, b::RED_CONCRETE),
    ]
}

fn column(x: i32, z: i32, height: i32, block: i32) -> impl Iterator<Item = Voxel> {
    (0..height).map(move |y| (x, y, z, block))
}

/// The blocks of a landmark around its marker.
pub fn landmark(kind: MarkerKind) -> Vec<Voxel> {
    match kind {
        MarkerKind::Village => [house(-3, -2), house(1, -3), house(-1, 1)].concat(),
        MarkerKind::Ruins => [(-2, -2, 3), (2, -2, 1), (-2, 2, 2), (2, 2, 3), (0, -3, 2)]
            .into_iter()
            .flat_map(|(x, z, height)| column(x, z, height, b::LIGHT_GRAY_CONCRETE))
            .collect(),
        MarkerKind::Mine => {
            let posts =
                column(-2, 0, 3, b::BROWN_TERRACOTTA).chain(column(2, 0, 3, b::BROWN_TERRACOTTA));
            let beam = (-2..=2).map(|x| (x, 3, 0, b::BROWN_TERRACOTTA));
            let pit = (-1..=1).map(|x| (x, 0, 1, b::BLACK_CONCRETE));
            posts.chain(beam).chain(pit).collect()
        }
        MarkerKind::Lighthouse => (0..6)
            .map(|y| {
                let band = if y % 2 == 0 {
                    b::WHITE_CONCRETE
                } else {
                    b::RED_CONCRETE
                };
                (0, y, 0, band)
            })
            .chain([(0, 6, 0, b::YELLOW_CONCRETE)])
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_landmark_fits_its_site() {
        for kind in [
            MarkerKind::Village,
            MarkerKind::Ruins,
            MarkerKind::Mine,
            MarkerKind::Lighthouse,
        ] {
            let blocks = landmark(kind);
            assert!(!blocks.is_empty());
            assert!(
                blocks
                    .iter()
                    .all(|(x, _, z, _)| x.abs() <= 3 && z.abs() <= 3)
            );
        }
    }
}
