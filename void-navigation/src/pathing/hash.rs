use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
pub struct FastHasher(u64);

impl Hasher for FastHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(byte as u64);
        }
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.write_u64(value as u64);
    }

    #[inline]
    fn write_i32(&mut self, value: i32) {
        self.write_u64(value as u32 as u64);
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(SEED);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

pub type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;
