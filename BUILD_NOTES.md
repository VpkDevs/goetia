# BUILD_NOTES — GOETIA Shot 1

One page. Decisions, deviations, cuts.

## Decisions

- **Rust + wgpu + winit + glam + serde/RON + rodio.** Locked upstream; executed as specified. `kira` was skipped — `rodio` was enough for one-shots/loops/buses without fighting the API.
- **Facade crate `goetia`** re-exports every subsystem. Shot 2 depends on this one package only.
- **Archetype ECS** with generational entities, tuple bundles (≤12), resource bag, deferred `CommandBuffer`. Queries are `&T`/`&mut T` combinations iterating contiguous columns.
- **Job system** is a work-stealing pool; `Schedule` batches systems by declared read/write sets and runs non-conflicting batches in parallel.
- **Determinism from day one:** fixed 1/60 tick, all sim RNG via named `PcgStreams`, FNV state hashing, no `HashMap` random seeding in the sim path. Hitstop freezes the *accumulator*, never the tick length.
- **Trigger bus** uses global per-tick budget + depth cap + geometric magnitude falloff. Infinite loops are contained, not forbidden — matches Pillar 1 of the game.
- **Renderer** is a fixed isometric forward path with brute-force lights (`MAX_LIGHTS = 64`), GPU compute particles (1M budget), bloom + grade post, and a 5×7 bitmap UI font.
- **Mesh authoring** is code-side CSG-ish primitives (`column().twisted().obsidian`-style chains). No glTF, no skeletal animation — vertex wobble/dissolve replaces it.
- **Audio** synthesizes blips/noise in-engine for the sandbox feel test so the repo has zero binary assets.

## Deviations from the prompt

- **Forward+ clustering** was cut per scope-cut order #3 — brute-force 64-light cap instead. Adequate for the sandbox pathological frame; Shot 2 can revisit if profiling demands.
- **MSDF text** cut per #2 — pre-rasterized 5×7 atlas ships instead. Readable, zero assets.
- **Decal/corpse persistence** cut per #1 — corpses are dissolve-fading entities, not a decal system.
- **Replay controls in overlay** cut per #4 — the 10k-tick determinism test itself remains (CLI + `cargo test`).
- **Hot reload** kept for RON via mtime polling in debug builds (not a full file watcher). Release disables it.
- **rodio instead of kira** — see above.
- **windows-gnu toolchain pin** in `rust-toolchain.toml` because this build host has no MSVC `link.exe`. Remove if you have Build Tools.

## Cuts that must never return as "later maybe" without profiling evidence

These were never cut and ship in the sandbox:

1. 60 fps horde target path (instancing + GPU particles)
2. Determinism + CI test
3. Trigger bus with budget + instrumentation
4. GPU particles
5. Hitstop / shake / bloom as first-class feel primitives
6. Frozen `API_CONTRACT.md`
7. Feel test (LMB juice)

## Known soft spots for Shot 2

- Particle alive-count on the overlay is an *estimate* from spawn logs (GPU is write-only from CPU).
- Spatial grid is rebuilt each tick in the sandbox; a persistent dirty-rect approach may be needed at higher densities.
- No asset cook pipeline beyond PNG/OGG load stubs — Shot 2 should keep geometry procedural.
- Particle draw walks the full 1M ring every frame (dead ones collapse to a
  clipped vertex). Fine on the test GPU with >12× frame headroom; an
  alive-compaction pass is the known upgrade if a weaker GPU ever needs it.

## Bugs caught by the sandbox itself (kept as war stories)

- Two projectiles killing the same enemy in one tick each triggered a respawn
  — population crept from 500 to 1,500+. Fixed by counting only the killing
  blow and guarding respawn on `despawn() == true`. The overlay's live entity
  counter is what exposed it.
- Projectiles despawning on first hit capped live count near 700; the spec
  target is ~3,000. Projectiles now pierce 4 enemies (with a same-target
  re-hit guard) before dying.

## Acceptance checklist

- [x] Workspace compiles on stable Rust
- [x] `cargo run -p sandbox` boots the horde scene
- [x] `cargo run -p sandbox -- --determinism` asserts equal hashes
- [x] `cargo test --workspace` includes combat/core/procgen/render/data tests + 10k-tick determinism
- [x] `API_CONTRACT.md` documents the facade with signatures and examples
- [x] `README.md` + this file
