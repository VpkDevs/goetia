# GOETIA Engine

Custom, desktop-native, data-oriented game engine purpose-built for **DEMONICON**
— an isometric occult ARPG defined by enemy hordes, cascading proc chains, seeded
procedural realms, and heavy stylized VFX.

This is **Shot 1 of 2**. Shot 2 (the game) builds against the frozen
[`API_CONTRACT.md`](./API_CONTRACT.md) and must not modify engine internals.

## Requirements

- Rust stable (workspace pins `stable-x86_64-pc-windows-gnu` via `rust-toolchain.toml`
  for machines without MSVC Build Tools; remove that file if you prefer MSVC)
- A GPU with Vulkan or DX12 support (wgpu)
- Windows / Linux / macOS desktop

## Build & run

```bash
cd goetia
cargo build --workspace
cargo run -p sandbox                  # horde stress + feel test (windowed)
cargo run -p sandbox -- --determinism # 10k-tick headless equality check
cargo run -p sandbox -- --proc-chain  # trigger-budget containment scene
cargo run -p sandbox -- --realm       # room-grammar flythrough
cargo run -p sandbox -- --bench 600   # N frames then print bench report
cargo test --workspace                # unit + determinism CI tests
```

Release (for honest FPS numbers):

```bash
cargo run -p sandbox --release -- --bench 1200
```

## Sandbox controls

| Input | Action |
|-------|--------|
| `F1` | Toggle debug overlay |
| `LMB` | Feel-shot (hitstop + shake + bloom + sfx + floater) |
| Scroll | Zoom |
| `1` / `2` / `3` | Horde / Proc-chain / Realm scenes |
| `Esc` | Quit |

## Workspace layout

```
goetia/
  crates/
    goetia_core/     # archetype ECS, jobs, PCG, fixed clock
    goetia_combat/   # stats, status, trigger bus, spatial grid
    goetia_procgen/  # room grammar, weighted tables
    goetia_data/     # RON registry + hot reload, atomic save
    goetia_audio/    # rodio buses / one-shots / loops
    goetia_render/   # wgpu instancing, GPU particles, bloom, UI
    goetia/          # facade — the only crate Shot 2 depends on
  sandbox/           # acceptance stress scenes
  API_CONTRACT.md
  BUILD_NOTES.md
```

## What the sandbox proves

| Test | Claim |
|------|--------|
| **Horde** | 500 AI agents, ~3k live projectiles, ambient ~200k GPU particles, ~50 lights, corpses — target 60 fps on mid-range GPU |
| **Proc-chain** | Deliberate infinite trigger loop; budget contains it; overlay shows chain stats |
| **Determinism** | 10,000 ticks from one seed, twice, bit-identical world hash (`cargo test -p sandbox` or `--determinism`) |
| **Realm** | 6 dummy room templates → 15-room connected layout; same seed → identical hash; lit flythrough |
| **Feel** | LMB: muzzle light, trail particles, hitstop, shake, bloom, damage number, sound |

## Benchmark results

Measured on the build machine — **Ryzen 5 5600X + Radeon RX 6800 XT**,
1600×900, horde scene at steady state (500 enemies, ~3,100 live projectiles,
~200k live particles, 51 lights, corpses accumulating):

```
> cargo run -p sandbox --release -- --bench 1200 --novsync
--- goetia bench report ---
frames:        1200
avg frame:     1.31 ms (764 fps)
window max:    2.73 ms
worst ever:    2.75 ms
frames >20ms:  0 of 1200
```

With vsync on it locks flat at 16.7 ms. A 6800 XT is above the RTX 3060
target class, but a >12× headroom margin (1.3 ms vs the 16.7 ms budget)
comfortably covers the gap; the >20 ms counter excludes the first 60 frames
(pipeline compilation) and is rendered live on the overlay. The 10,000-tick
determinism test runs headless in ~10 s (`cargo test -p sandbox`).

## Design in one paragraph

Fixed-tick 60 Hz sim, interpolated render. Archetype ECS with a work-stealing
scheduler. GPU-driven instanced draws + compute particle sim. Combat is
circle queries + a budgeted trigger bus (infinite proc loops are content, not
bugs). All game data is RON; all sim RNG is named PCG streams. See
`API_CONTRACT.md` for the frozen surface Shot 2 builds against.
