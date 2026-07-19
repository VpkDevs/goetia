# GOETIA API Contract (Shot 1 — frozen)

Shot 2 builds against the `goetia` facade crate **only**. Do not depend on
internal crate paths (`goetia_core`, `goetia_render`, …) from game code.

This document freezes the public surface. Signatures match the code; examples
are the preferred usage pattern.

---

## App bootstrap & game trait

```rust
use goetia::prelude::*;

struct MyGame;

impl Game for MyGame {
    fn init(&mut self, eng: &mut Engine, gfx: Option<&mut Renderer>) {
        // Register meshes only when gfx is Some (windowed). Headless CI has None.
        if let Some(r) = gfx {
            let _ = r.register_mesh(MeshBuilder::cube());
        }
    }
    fn fixed_update(&mut self, eng: &mut Engine) {
        // Exactly 60 Hz sim. Mutate world here only.
        eng.run_schedule();
        eng.triggers.process(|ev, em| { /* reactions */ let _ = (ev, em); });
    }
    fn render_extract(&mut self, eng: &mut Engine, frame: &mut FrameSubmit, alpha: f32) {
        // Fill instances / lights / particles / UI. alpha interpolates prev→curr tick.
        let _ = (eng, frame, alpha);
    }
    fn ui(&mut self, _eng: &mut Engine, _ui: &mut UiBatch) {}
    fn on_event(&mut self, _eng: &mut Engine, _ev: &WindowEvent) {}
}

fn main() {
    App::run(AppConfig::default(), MyGame).unwrap();
}

// Headless (CI / determinism):
// let eng = App::run_headless(MyGame, seed, ticks);
```

### `AppConfig`
| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `title` | `String` | `"GOETIA"` | Window title |
| `size` | `(u32,u32)` | `(1600,900)` | Logical pixels |
| `vsync` | `bool` | `true` | |
| `max_frames` | `Option<u64>` | `None` | Exit + print bench after N frames |
| `master_seed` | `u64` | `0x60471A00D3E00666` | Seeds all PCG streams |
| `threads` | `usize` | `0` (= auto) | Job pool size |

### `Engine` (the bag)
| Field | Type | Purpose |
|-------|------|---------|
| `world` | `World` | Archetype ECS |
| `schedule` | `Schedule` | Parallel system schedule |
| `jobs` | `JobPool` | Work-stealing pool |
| `clock` | `GameClock` | Fixed tick + hitstop |
| `streams` | `PcgStreams` | Named deterministic RNGs |
| `triggers` | `TriggerBus` | Budgeted proc bus |
| `audio` | `AudioEngine` | One-shots / loops / buses |
| `input` | `Input` | Keys/mouse edges |
| `camera` | `CameraRig` | Iso camera + shake |
| `commands` | `CommandBuffer` | Deferred spawn/despawn |
| `overlay` | `Overlay` | Debug HUD (F1) |
| `floaters` | `DamageNumbers` | World-space damage text |
| `viewport` | `Vec2` | Window size px |
| `quit` | `bool` | Exit app loop |

### Engine helpers
```rust
eng.run_schedule();          // run registered systems (parallel where safe)
eng.hitstop(0.05);           // freeze sim for 50ms real time
eng.shake(0.3);              // trauma-based screenshake
let g: Vec3 = eng.mouse_ground(); // cursor → y=0 plane
```

### Timing constants
```rust
pub const TICK_RATE: f64 = 60.0;
pub const FIXED_DT: f32 = 1.0 / 60.0;
```

---

## ECS

```rust
#[derive(Clone, Copy)]
struct Pos(pub Vec2);
#[derive(Clone, Copy)]
struct Hp(pub f32);

let e = eng.world.spawn((Pos(Vec2::ZERO), Hp(100.0)));
eng.world.get::<Hp>(e);
eng.world.get_mut::<Hp>(e);
eng.world.insert(e, StatusBag::new());
eng.world.remove::<StatusBag>(e);
eng.world.despawn(e);
eng.world.is_alive(e);

// Iterate matching archetypes (slice-backed, no per-entity alloc):
eng.world.each::<(&Pos, &mut Hp)>(|entity, (pos, hp)| {
    let _ = (entity, pos, hp);
});

// Resources
eng.world.insert_resource(SpatialGrid::new(2.0));
let g = eng.world.resource::<SpatialGrid>();
let g = eng.world.resource_mut::<SpatialGrid>();

// Deferred structural ops (applied after fixed_update by App):
eng.commands.spawn((Pos(Vec2::ONE), Hp(10.0)));
eng.commands.despawn(e);

// Parallel schedule
eng.schedule.add(
    SystemDef::new("move", |w| {
        w.each::<(&mut Pos,)>(|_, (p,)| p.0 += Vec2::X * FIXED_DT);
    })
    .writes::<Pos>(),
);
eng.run_schedule();
```

- Components: any `'static + Send + Sync` type. Bundles: tuples up to 12.
- `Entity { index, gen }` — generational; stale IDs fail `is_alive` / `get`.
- Queries: combinations of `&T` / `&mut T` (and unit tuples).

### Determinism hashing
```rust
let mut h = StateHasher::new();
h.write_u64(eng.clock.tick);
h.write_f32(x);
h.write_vec2(x, y);
let fingerprint = h.finish(); // bit-exact across runs
```

---

## Stat sheets & modifiers

```rust
let mut sheet = StatSheet::new()
    .with(StatKey::of("max_hp"), 100.0)
    .with(StatKey::of("damage"), 10.0);

let h = sheet.add_modifier(StatKey::of("damage"), ModOp::Add, 5.0);
sheet.add_modifier(StatKey::of("damage"), ModOp::Mul, 0.20); // +20% increased
// Override wins outright (highest priority):
sheet.add_modifier_prio(StatKey::of("damage"), ModOp::Override, 999.0, 10);

let dmg = sheet.get(StatKey::of("damage")); // lazy recompute per key
sheet.remove_modifier(h);
```

`ModOp::{Add, Mul, Override}` — final = `(base + ΣAdd) * (1 + ΣMul)`, or Override.

---

## Status lifecycle

```rust
let mut reg = StatusRegistry::default();
let id = reg.register(StatusDef {
    name: "ignite".into(),
    max_stacks: 5,
    duration_ticks: 180,
    tick_interval: 30,
    refresh_on_apply: true,
});

let mut bag = StatusBag::new();
let mut events = Vec::new();
bag.apply(entity, reg.get(id).unwrap(), 1, 4.0, &mut events);
bag.tick(entity, &reg, &mut events);
bag.detonate(entity, id, &mut events); // consume → Detonated event
// bag.remove(id); // silent dispel

// StatusEvent::{Applied, Ticked, Expired, Detonated}
// StatusId::of("ignite") == id
```

Game content names statuses; the engine only runs apply/stack/tick/expire/detonate.

---

## Trigger bus

```rust
eng.triggers.config = TriggerConfig {
    budget_per_tick: 2048,
    max_chain_depth: 16,
    chain_falloff: 0.7,   // magnitude *= falloff^(depth+1) on child emits
    magnitude_floor: 0.05,
};

eng.triggers.emit(TriggerKind::of("on_kill"), src, tgt, 1.0);

eng.triggers.process(|ev, em| {
    // React; chain further with em.emit (inherits depth+1 + falloff).
    if ev.kind == TriggerKind::of("on_kill") {
        em.emit(TriggerKind::of("on_crit"), ev.source, ev.target, ev.magnitude);
    }
});

// Instrumentation (also drawn on overlay):
// eng.triggers.stats.{emitted, processed, dropped_budget, dropped_depth,
//                     dropped_floor, max_depth_seen, budget_hit_ticks,
//                     last_tick_processed}
```

**Contract:** infinite loops are *contained*, never forbidden. Budget exhaustion
drops remaining events for the tick and increments `budget_hit_ticks`. Do not
add "X can't proc Y" exceptions at the engine layer.

---

## Spatial queries

```rust
let mut grid = SpatialGrid::new(/* cell size */ 2.0);
grid.clear();
grid.insert(entity, pos, radius, /* mask bitfield */ 0b01);

let mut hits = Vec::new();
grid.query_radius(center, 5.0, 0b01, &mut hits); // (Entity, Vec2)

grid.query_cone(origin, dir, half_angle_rad, range, mask, &mut hits);

if let Some((ent, at, t)) = grid.sweep(from, to, proj_radius, mask) {
    // nearest circle hit along segment; t in [0,1]
    let _ = (ent, at, t);
}
```

Combat physics is circle-vs-circle + swept circles only. No rigid-body solver.

---

## Particles

```rust
frame.particle_spawns.push(ParticleSpawn {
    pos: Vec3::new(0.0, 1.0, 0.0),
    count: 64,
    vel: Vec3::Y * 2.0,
    spread: 4.0,                 // random sphere kick
    color_from: Vec4::new(1.0, 0.6, 0.2, 1.0),
    color_to:   Vec4::new(1.0, 0.1, 0.0, 0.0),
    size: (0.05, 0.14),
    life: (0.3, 0.9),
    gravity: 6.0,
    drag: 1.5,
});
// Capacity: 1_048_576 GPU particles. CPU never touches them after spawn.
// frame.particle_dt is set by App (hitstop-aware).
```

---

## Lights / materials / instances

```rust
frame.lights.push(Light {
    pos: Vec3::new(0.0, 2.0, 0.0),
    color: palette::BRIMSTONE,
    radius: 6.0,
    intensity: 2.0,
});
// Cap: MAX_LIGHTS = 64 (brightest-nearest wins when over budget).

let inst = InstanceRaw::new(model_mat4, Vec4::new(0.8, 0.7, 0.6, 1.0))
    .emissive(palette::HEX, 1.5)
    .phase(0.37)           // vertex-shader seed
    .wobble(0.05, 3.0)     // procedural motion (replaces skeletal anim)
    .dissolve(0.0);        // 0..1 death dissolve

frame.meshes.push((mesh_handle, vec![inst]));
frame.ambient = Vec3::new(0.10, 0.09, 0.13);
frame.fog = Vec4::new(0.03, 0.02, 0.05, /* density */ 0.018);
frame.bloom = 1.0;
```

### Palette (engine-owned, do not recolor casually)
```rust
palette::{VOID, ASH, BONE, BRIMSTONE, HEX, ICHOR, BLOOD, GOLD}
```

### Mesh authoring
```rust
let h = renderer.register_mesh(
    MeshBuilder::column(2.0)
        .twisted(0.15)
        .tapered(0.7)
        .jittered(0.03)
        .merged(MeshBuilder::orb(3, 0.4).translated(Vec3::Y * 2.0))
);
// Primitives: cube, boxed, column, prism, orb, ground, spike
// Transforms: transformed / translated / scaled / rotated / twisted / tapered / jittered / merged
```

---

## Camera, shake, hitstop

```rust
eng.camera.target = Vec3::new(0.0, 0.0, 0.0); // look-at on ground plane
eng.camera.zoom = 18.0;                       // clamped to [zoom_min, zoom_max]
eng.camera.zoom_by(0.9);
eng.shake(0.25);   // trauma 0..1; amplitude = trauma²
eng.hitstop(0.04); // freezes sim accumulator (not tick length)
```

Fixed isometric angle. No free camera.

---

## UI draw API

```rust
frame.ui.rect(pos_px, size_px, color_rgba);
frame.ui.text(pos, scale, color, "HELLO");
frame.ui.text_shadowed(pos, scale, color, "HELLO");
let w = UiBatch::text_width(scale, "HELLO");
// Glyphs: 5×7 bitmap atlas (MSDF fallback). Coordinates are pixels, top-left origin.
```

### Damage numbers
```rust
eng.floaters.spawn(world_pos, "12", Vec4::ONE, /* scale */ 1.5);
// Updated/drawn by App; ignore hitstop so juice still lands.
```

---

## Audio

```rust
let sfx = Sound::blip(440.0, 0.08);
// or Sound::load("res/hit.ogg") / Sound::noise_burst(0.1, 200.0) / Sound::synth(...)
eng.audio.set_bus("sfx", 0.8);
eng.audio.play(&sfx, "sfx", /* volume */ 1.0, /* pitch */ 1.0);
let h = eng.audio.play_loop(&music, "music", 0.5);
eng.audio.set_loop_volume(h, 0.2);
eng.audio.stop_loop(h);
// Headless: AudioEngine::disabled() — play is a no-op.
```

---

## RON data registry + hot reload

```rust
let mut reg: DataRegistry<StatusDef> = DataRegistry::new();
// reg.hot_reload is true in debug builds.
let h = reg.load("data/statuses/ignite.ron")?;
let def: &StatusDef = reg.get(&h);
// Once per second (or per frame in debug):
let n = reg.poll_reload(); // swaps in-place on mtime change; bad RON keeps old value
let _ = reg.version;       // bumps on every successful (re)load
```

---

## PCG streams

```rust
// Created from master_seed at Engine::new.
let layout = eng.streams.get("layout"); // &mut Pcg32
let x = layout.range_f32(0.0, 1.0);
let i = layout.range_u32(10);
// Named streams are independent: rolling "loot" never advances "layout".
// Common names used by the sandbox / expected by Shot 2: layout, loot, packs, naming, ai
```

```rust
// Weighted tables with pity:
let mut table = WeightedTable::new();
table.push("common", 10.0, 0.0).push("rare", 1.0, 0.15);
let item = table.roll(eng.streams.get("loot"));
```

---

## Room-grammar input format

```ron
// RoomTemplate
(
    name: "hub",
    width: 4,
    height: 4,
    doors: [
        (side: North, offset: 1),
        (side: South, offset: 2),
    ],
    tags: ["hub"],
    weight: 1.0,
)

// RealmGrammar
(
    start: "hub",
    target_rooms: 15,
    allow: [], // empty = all tags connect; else list of (tag_a, tag_b) pairs
)
```

```rust
let layout = assemble(&templates, &grammar, eng.streams.get("layout")).unwrap();
// layout.rooms: Vec<PlacedRoom { template, x, y }>
// layout.connections: Vec<(room_a, room_b, door_cell)>
// layout.hash() — stable fingerprint for determinism tests
```

---

## Save API

```rust
let mut save = SaveFile::new();
save.put("banked_loot", &my_loot_vec)?;
save.put("unlocks", &unlock_flags)?;
save.write("saves/slot0.sav")?; // write-temp-then-rename (atomic, corruption-safe)

let save = SaveFile::read("saves/slot0.sav")?;
let loot: Vec<ItemId> = save.take("banked_loot")?;
```

Versioned serde blob store. Engine does not auto-save; the game decides when.

---

## Debug overlay extension points

```rust
eng.overlay.enabled = true; // F1 toggles in App
eng.overlay.lines.push(format!("tier {}", tier)); // cleared after draw each frame
// Built-in: FPS graph, entity/archetype counts, trigger stats, draw/instance/
// light/particle counts, system timings, frames >20ms counter.
let (avg_ms, max_ms, worst_ms) = eng.overlay.stats();
```

---

## Prelude

```rust
use goetia::prelude::*;
// Re-exports: App, Engine, Game, World, Entity, all combat/procgen/render/audio
// types above, plus glam::{Mat4, Quat, Vec2, Vec3, Vec4}, winit KeyCode / events.
```

---

## What is intentionally not in the contract

- Scene graph editors, skeletal animation, glTF import
- General physics / networking / scripting language
- Arbitrary cameras, console/mobile targets
- Direct access to wgpu device (use `Renderer` / `FrameSubmit` only)

If Shot 2 needs something missing here, that is a Shot 1 defect — extend this
contract in a deliberate revision, do not fork engine internals mid-game build.
