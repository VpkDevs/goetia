//! The determinism acceptance test: 10,000 fixed ticks of the full horde sim
//! (500 AI enemies, ~3000 projectiles, deaths, respawns, RNG streams), run
//! twice from the same seed, hashed bit-for-bit. Headless — no window, no
//! GPU — so it runs in CI.

use sandbox::run_headless_horde;

#[test]
fn horde_10k_ticks_bit_identical() {
    let a = run_headless_horde(0xD3AD_B33F, 10_000);
    let b = run_headless_horde(0xD3AD_B33F, 10_000);
    assert_eq!(a, b, "same seed diverged: {a:016X} vs {b:016X}");
}

#[test]
fn different_seeds_diverge() {
    let a = run_headless_horde(1, 500);
    let b = run_headless_horde(2, 500);
    assert_ne!(
        a, b,
        "different seeds produced identical worlds (hash collision or dead RNG)"
    );
}
