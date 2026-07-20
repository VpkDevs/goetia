//! PCG32 random streams. All simulation randomness flows through named
//! streams so that e.g. an extra loot roll never perturbs level layout.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// PCG-XSH-RR 64/32 (O'Neill). Deterministic, fast, serializable.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut rng = Pcg32 {
            state: 0,
            inc: (stream << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        ((self.next_u32() as u64) << 32) | self.next_u32() as u64
    }

    /// Uniform in [0, 1).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / (1 << 24) as f32)
    }

    /// Uniform in [lo, hi).
    #[inline]
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next_f32() * (hi - lo)
    }

    /// Uniform in [0, n) without modulo bias (Lemire).
    #[inline]
    pub fn range_u32(&mut self, n: u32) -> u32 {
        debug_assert!(n > 0);
        let mut x = self.next_u32();
        let mut m = (x as u64) * (n as u64);
        let mut l = m as u32;
        if l < n {
            let t = n.wrapping_neg() % n;
            while l < t {
                x = self.next_u32();
                m = (x as u64) * (n as u64);
                l = m as u32;
            }
        }
        (m >> 32) as u32
    }

    #[inline]
    pub fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        debug_assert!(hi > lo);
        lo + self.range_u32((hi - lo) as u32) as i32
    }

    #[inline]
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    /// Pick an index from a slice of weights. Returns None on empty/zero-sum.
    pub fn weighted_index(&mut self, weights: &[f32]) -> Option<usize> {
        let total: f32 = weights.iter().sum();
        if total <= 0.0 {
            return None;
        }
        let mut roll = self.next_f32() * total;
        for (i, w) in weights.iter().enumerate() {
            roll -= w;
            if roll < 0.0 {
                return Some(i);
            }
        }
        Some(weights.len() - 1)
    }

    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        for i in (1..slice.len()).rev() {
            let j = self.range_u32(i as u32 + 1) as usize;
            slice.swap(i, j);
        }
    }
}

/// Named, independently-seeded streams. The stream id is derived from the
/// name's FNV hash, so stream identity survives reordering of creation.
#[derive(Serialize, Deserialize, Clone)]
pub struct PcgStreams {
    master_seed: u64,
    streams: BTreeMap<String, Pcg32>,
}

impl PcgStreams {
    pub fn new(master_seed: u64) -> Self {
        PcgStreams {
            master_seed,
            streams: BTreeMap::new(),
        }
    }

    pub fn master_seed(&self) -> u64 {
        self.master_seed
    }

    pub fn get(&mut self, name: &str) -> &mut Pcg32 {
        if !self.streams.contains_key(name) {
            let stream_id = crate::hash::fnv1a64(name.as_bytes());
            self.streams
                .insert(name.to_string(), Pcg32::new(self.master_seed, stream_id));
        }
        self.streams.get_mut(name).unwrap()
    }

    /// Fork a child RNG (e.g. per-room, per-entity) without touching the
    /// parent stream's future output more than one draw.
    pub fn fork(&mut self, name: &str) -> Pcg32 {
        let s = self.get(name);
        let seed = s.next_u64();
        Pcg32::new(seed, seed ^ 0x9E3779B97F4A7C15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_independent() {
        let mut a = PcgStreams::new(42);
        let mut b = PcgStreams::new(42);
        // Draw from loot in `a` before layout; layout must be unaffected.
        let _ = a.get("loot").next_u32();
        let la: Vec<u32> = (0..8).map(|_| a.get("layout").next_u32()).collect();
        let lb: Vec<u32> = (0..8).map(|_| b.get("layout").next_u32()).collect();
        assert_eq!(la, lb);
        let mut c = PcgStreams::new(43);
        let lc: Vec<u32> = (0..8).map(|_| c.get("layout").next_u32()).collect();
        assert_ne!(la, lc);
    }

    #[test]
    fn range_bounds() {
        let mut r = Pcg32::new(7, 1);
        for _ in 0..10_000 {
            let v = r.range_u32(13);
            assert!(v < 13);
            let f = r.next_f32();
            assert!((0.0..1.0).contains(&f));
        }
    }
}
