//! Deterministic state hashing for the determinism test / replay validation.

use std::hash::{BuildHasherDefault, Hasher};

pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
pub const FNV_PRIME: u64 = 0x100000001b3;

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Deterministic (unseeded) FNV-1a hasher for engine-internal maps. Std's
/// SipHash is randomly seeded per process — useless when we want identical
/// behavior across runs.
#[derive(Default, Clone)]
pub struct Fnv64Hasher(u64);

impl Hasher for Fnv64Hasher {
    fn finish(&self) -> u64 {
        if self.0 == 0 {
            FNV_OFFSET
        } else {
            self.0
        }
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { FNV_OFFSET } else { self.0 };
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        self.0 = h;
    }
}

pub type FnvBuildHasher = BuildHasherDefault<Fnv64Hasher>;
pub type FnvHashMap<K, V> = std::collections::HashMap<K, V, FnvBuildHasher>;
pub type FnvHashSet<K> = std::collections::HashSet<K, FnvBuildHasher>;

/// Order-sensitive incremental hasher. Feed sim state in a canonical order
/// (e.g. iterate a query, write entity id then fields) to get a bit-exact
/// fingerprint of the world.
#[derive(Clone)]
pub struct StateHasher {
    h: u64,
}

impl Default for StateHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl StateHasher {
    pub fn new() -> Self {
        StateHasher { h: FNV_OFFSET }
    }
    #[inline]
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.h ^= b as u64;
            self.h = self.h.wrapping_mul(FNV_PRIME);
        }
    }
    #[inline]
    pub fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }
    #[inline]
    pub fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_le_bytes());
    }
    #[inline]
    pub fn write_i32(&mut self, v: i32) {
        self.write_bytes(&v.to_le_bytes());
    }
    /// Hashes the exact bit pattern — NaN payloads and -0.0 vs 0.0 matter,
    /// which is precisely what we want for bit-for-bit determinism checks.
    #[inline]
    pub fn write_f32(&mut self, v: f32) {
        self.write_u32(v.to_bits());
    }
    #[inline]
    pub fn write_vec2(&mut self, x: f32, y: f32) {
        self.write_f32(x);
        self.write_f32(y);
    }
    pub fn finish(&self) -> u64 {
        self.h
    }
}
