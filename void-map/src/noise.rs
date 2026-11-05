//! Smooth deterministic noise, for what the world generator leaves too regular.

fn hash(seed: u32, x: i32, z: i32) -> f64 {
    let mut n = seed
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((x as u32).wrapping_mul(0x85EB_CA77))
        .wrapping_add((z as u32).wrapping_mul(0xC2B2_AE3D));
    n ^= n >> 15;
    n = n.wrapping_mul(0x2C1B_3C6D);
    n ^= n >> 12;
    n = n.wrapping_mul(0x297A_2D39);
    n ^= n >> 15;
    f64::from(n) / f64::from(u32::MAX) * 2.0 - 1.0
}

fn value(seed: u32, x: f64, z: f64) -> f64 {
    let (cell_x, cell_z) = (x.floor(), z.floor());
    let ease = |t: f64| t * t * (3.0 - 2.0 * t);
    let (tx, tz) = (ease(x - cell_x), ease(z - cell_z));
    let (ix, iz) = (cell_x as i32, cell_z as i32);
    let north = hash(seed, ix, iz) * (1.0 - tx) + hash(seed, ix + 1, iz) * tx;
    let south = hash(seed, ix, iz + 1) * (1.0 - tx) + hash(seed, ix + 1, iz + 1) * tx;
    north * (1.0 - tz) + south * tz
}

/// Noise around zero, within ±1, whose features are about `size` wide.
pub fn fractal(seed: u32, x: f64, z: f64, size: f64) -> f64 {
    let (mut sum, mut amplitude, mut frequency, mut total) = (0.0, 1.0, 1.0 / size, 0.0);
    for octave in 0..4 {
        sum += value(seed + octave, x * frequency, z * frequency) * amplitude;
        total += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    sum / total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_place_gives_the_same_noise() {
        assert_eq!(fractal(1, 12.5, -3.0, 40.0), fractal(1, 12.5, -3.0, 40.0));
    }

    #[test]
    fn seeds_differ() {
        assert_ne!(fractal(1, 12.5, -3.0, 40.0), fractal(2, 12.5, -3.0, 40.0));
    }

    #[test]
    fn noise_stays_within_its_range_and_moves_smoothly() {
        for step in 0..2_000 {
            let x = f64::from(step) * 0.7;
            let here = fractal(3, x, x * 0.3, 80.0);
            assert!(here.abs() <= 1.0);
            assert!((here - fractal(3, x + 0.1, x * 0.3, 80.0)).abs() < 0.1);
        }
    }
}
