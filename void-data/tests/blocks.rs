//! Smoke tests for the build-script-generated `v26_1_2::blocks` and
//! `v26_1_2::state` modules. Verifies that:
//!   * Constant ids match the Mojang report.
//!   * `OakStairs::DEFAULT.to_state_id() == DEFAULT_STATE_ID` and the constant
//!     value lines up.
//!   * `from_state_id(to_state_id(x)) == Some(x)` for every reachable state.
//!   * Out-of-range ids yield `None`.

use voidmc_data::v26_1_2::blocks;
use voidmc_data::v26_1_2::props::*;
use voidmc_data::v26_1_2::shapes;
use voidmc_data::v26_1_2::state;

#[test]
fn stone_default_is_one() {
    assert_eq!(blocks::STONE, 1);
}

#[test]
fn air_default_is_zero() {
    assert_eq!(blocks::AIR, 0);
}

#[test]
fn oak_stairs_default_matches_const() {
    let s = state::OakStairs::DEFAULT;
    assert_eq!(s.to_state_id(), state::OakStairs::DEFAULT_STATE_ID);
    assert_eq!(blocks::OAK_STAIRS, s.to_state_id());
}

#[test]
fn oak_stairs_default_has_expected_props() {
    let s = state::OakStairs::DEFAULT;
    assert_eq!(s.facing, Facing4::North);
    assert_eq!(s.shape, Shape5::Straight);
    assert!(!s.waterlogged);
}

#[test]
fn oak_stairs_round_trip_full_range() {
    for id in state::OakStairs::MIN_STATE_ID..=state::OakStairs::MAX_STATE_ID {
        let s = state::OakStairs::from_state_id(id).expect("in range");
        assert_eq!(s.to_state_id(), id, "round-trip mismatch at id={id}");
    }
    assert!(state::OakStairs::from_state_id(state::OakStairs::MIN_STATE_ID - 1).is_none());
    assert!(state::OakStairs::from_state_id(state::OakStairs::MAX_STATE_ID + 1).is_none());
}

#[test]
fn grass_block_round_trip() {
    for id in state::GrassBlock::MIN_STATE_ID..=state::GrassBlock::MAX_STATE_ID {
        let s = state::GrassBlock::from_state_id(id).expect("in range");
        assert_eq!(s.to_state_id(), id);
    }
}

#[test]
fn shape_for_air_is_empty() {
    let aabbs = shapes::for_state(blocks::AIR);
    assert!(aabbs.is_empty(), "air should have no collision boxes");
}

#[test]
fn shape_for_stone_is_full_cube() {
    let aabbs = shapes::for_state(blocks::STONE);
    assert_eq!(aabbs.len(), 1);
    let b = aabbs[0];
    assert_eq!(
        (b.x0, b.y0, b.z0, b.x1, b.y1, b.z1),
        (0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
    );
}

#[test]
fn shape_for_oak_stairs_is_non_full_cube() {
    // Stair shapes decompose into multiple AABBs (bottom slab + step) — the
    // exact count varies by orientation / inner-corner shape, but it should
    // always be smaller than a full cube and never empty.
    let aabbs = shapes::for_state(blocks::OAK_STAIRS);
    assert!(!aabbs.is_empty(), "stairs must have at least one AABB");
    let is_full_cube = aabbs.len() == 1
        && aabbs[0].x0 == 0.0
        && aabbs[0].y0 == 0.0
        && aabbs[0].z0 == 0.0
        && aabbs[0].x1 == 1.0
        && aabbs[0].y1 == 1.0
        && aabbs[0].z1 == 1.0;
    assert!(!is_full_cube, "stairs must not be a full cube");
}
