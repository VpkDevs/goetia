//! GOETIA engine sandbox — the acceptance test for Shot 1.
//!
//! ```text
//! cargo run -p sandbox                  # horde + feel (windowed)
//! cargo run -p sandbox -- --determinism # 10k-tick headless CI check
//! cargo run -p sandbox -- --proc-chain  # deliberate infinite-loop containment
//! cargo run -p sandbox -- --realm       # room-grammar flythrough
//! cargo run -p sandbox -- --bench 600   # N frames then print report
//! ```

use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use goetia::prelude::*;
use sandbox::horde::{self, FxQueue, FIRE_PER_TICK};
use sandbox::{AiState, Corpse, Enemy, Pos, PrevPos, Projectile, Vel};
use std::env;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mode = args.first().map(|s| s.as_str()).unwrap_or("");

    match mode {
        "--determinism" | "determinism" => run_determinism(),
        "--proc-chain" | "proc-chain" => {
            if args.iter().any(|a| a == "--headless") {
                run_proc_chain_headless();
            } else {
                run_windowed(SceneMode::ProcChain, parse_bench(&args));
            }
        }
        "--realm" | "realm" => run_windowed(SceneMode::Realm, parse_bench(&args)),
        "--bench" => {
            let n = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(600);
            run_windowed(SceneMode::Horde, Some(n));
        }
        "--help" | "-h" | "help" => print_help(),
        _ => {
            let bench = parse_bench(&args);
            run_windowed(SceneMode::Horde, bench);
        }
    }
}

fn parse_bench(args: &[String]) -> Option<u64> {
    args.windows(2)
        .find(|w| w[0] == "--bench")
        .and_then(|w| w[1].parse().ok())
}

fn print_help() {
    println!(
        "GOETIA sandbox\n\
         \n\
         cargo run -p sandbox                  horde stress + feel test\n\
         cargo run -p sandbox -- --determinism headless 10k-tick equality\n\
         cargo run -p sandbox -- --proc-chain  trigger-budget containment\n\
         cargo run -p sandbox -- --realm       room-grammar flythrough\n\
         cargo run -p sandbox -- --bench 600   exit after N frames\n\
         \n\
         In-window: F1 overlay · LMB feel-shot · scroll zoom · Esc quit\n\
         1 horde · 2 proc-chain · 3 realm"
    );
}

// ---------------------------------------------------------------- determinism

fn run_determinism() {
    const SEED: u64 = 0xDEAD_BEEF_CAFE_F00D;
    const TICKS: u64 = 10_000;

    println!("determinism: {TICKS} ticks × 2, seed={SEED:#x}");
    let mut a = App::run_headless(HordeHeadless, SEED, TICKS);
    let ha = horde::world_hash(&mut a);
    let mut b = App::run_headless(HordeHeadless, SEED, TICKS);
    let hb = horde::world_hash(&mut b);

    println!("hash A: {ha:#018x}");
    println!("hash B: {hb:#018x}");
    if ha == hb {
        println!("PASS — bit-identical world state");
    } else {
        eprintln!("FAIL — world hashes diverged");
        std::process::exit(1);
    }
}

struct HordeHeadless;
impl Game for HordeHeadless {
    fn init(&mut self, eng: &mut Engine, _gfx: Option<&mut Renderer>) {
        horde::reset(eng);
    }
    fn fixed_update(&mut self, eng: &mut Engine) {
        horde::tick(eng);
    }
    fn render_extract(&mut self, _eng: &mut Engine, _frame: &mut FrameSubmit, _alpha: f32) {}
}

// ---------------------------------------------------------------- proc-chain headless

fn run_proc_chain_headless() {
    let mut eng = Engine::new(1, 1, true);
    eng.triggers.config = TriggerConfig {
        budget_per_tick: 512,
        max_chain_depth: 64,
        chain_falloff: 0.85,
        magnitude_floor: 0.01,
    };
    // Seed a deliberate A↔B infinite loop every tick for 120 ticks.
    for _ in 0..120 {
        let e = Entity { index: 0, gen: 0 };
        eng.triggers.emit(TriggerKind::of("on_kill"), e, e, 1.0);
        eng.triggers.process(|ev, em| {
            let next = if ev.kind == TriggerKind::of("on_kill") {
                TriggerKind::of("on_status_apply")
            } else if ev.kind == TriggerKind::of("on_status_apply") {
                TriggerKind::of("on_crit")
            } else {
                TriggerKind::of("on_kill")
            };
            em.emit(next, ev.source, ev.target, 1.0);
            em.emit(next, ev.source, ev.target, 1.0);
        });
    }
    let s = eng.triggers.stats;
    println!("proc-chain headless:");
    println!("  processed:        {}", s.processed);
    println!("  dropped_budget:   {}", s.dropped_budget);
    println!("  budget_hit_ticks: {}", s.budget_hit_ticks);
    println!("  max_depth_seen:   {}", s.max_depth_seen);
    assert!(
        s.budget_hit_ticks > 0,
        "budget never hit — loop not exploding"
    );
    assert_eq!(eng.triggers.pending(), 0, "queue leaked across ticks");
    println!("PASS — budget contained the infinite loop");
}

// ---------------------------------------------------------------- windowed scenes

#[derive(Clone, Copy, PartialEq, Eq)]
enum SceneMode {
    Horde,
    ProcChain,
    Realm,
}

fn run_windowed(mode: SceneMode, max_frames: Option<u64>) {
    let title = match mode {
        SceneMode::Horde => "GOETIA sandbox — HORDE (F1 overlay, LMB feel-shot)",
        SceneMode::ProcChain => "GOETIA sandbox — PROC CHAIN",
        SceneMode::Realm => "GOETIA sandbox — REALM",
    };
    let config = AppConfig {
        title: title.into(),
        size: (1600, 900),
        vsync: !std::env::args().any(|a| a == "--novsync"),
        max_frames,
        master_seed: 0x6047_1A00_D3E0_0666,
        threads: 0,
    };
    if let Err(e) = App::run(config, SandboxGame::new(mode)) {
        eprintln!("sandbox failed: {e}");
        std::process::exit(1);
    }
}

struct SandboxGame {
    mode: SceneMode,
    // meshes
    mesh_enemy: Option<MeshHandle>,
    mesh_proj: Option<MeshHandle>,
    mesh_corpse: Option<MeshHandle>,
    mesh_ground: Option<MeshHandle>,
    mesh_turret: Option<MeshHandle>,
    mesh_room: Option<MeshHandle>,
    mesh_column: Option<MeshHandle>,
    // feel-shot audio
    sfx_fire: Option<Sound>,
    sfx_hit: Option<Sound>,
    // realm
    realm: Option<RealmLayout>,
    templates: Vec<RoomTemplate>,
    // proc-chain dummy
    dummy_entities: Vec<Entity>,
    status_reg: StatusRegistry,
    // feel shot cooldown
    feel_cd: u16,
    // ambient particle budget tracking
    particle_pulse: u32,
}

impl SandboxGame {
    fn new(mode: SceneMode) -> Self {
        SandboxGame {
            mode,
            mesh_enemy: None,
            mesh_proj: None,
            mesh_corpse: None,
            mesh_ground: None,
            mesh_turret: None,
            mesh_room: None,
            mesh_column: None,
            sfx_fire: None,
            sfx_hit: None,
            realm: None,
            templates: Vec::new(),
            dummy_entities: Vec::new(),
            status_reg: StatusRegistry::default(),
            feel_cd: 0,
            particle_pulse: 0,
        }
    }

    fn switch(&mut self, eng: &mut Engine, gfx: Option<&mut Renderer>, mode: SceneMode) {
        self.mode = mode;
        eng.triggers = TriggerBus::default();
        eng.floaters = DamageNumbers::new();
        eng.camera = CameraRig::new();
        match mode {
            SceneMode::Horde => {
                horde::reset(eng);
                eng.camera.zoom = 22.0;
            }
            SceneMode::ProcChain => self.init_proc_chain(eng),
            SceneMode::Realm => self.init_realm(eng, gfx),
        }
    }

    fn init_proc_chain(&mut self, eng: &mut Engine) {
        eng.world = World::new();
        eng.schedule = Schedule::new();
        eng.clock.tick = 0;
        eng.triggers.config = TriggerConfig {
            budget_per_tick: 1024,
            max_chain_depth: 32,
            chain_falloff: 0.75,
            magnitude_floor: 0.02,
        };
        self.status_reg = StatusRegistry::default();
        self.status_reg.register(StatusDef {
            name: "ignite".into(),
            max_stacks: 8,
            duration_ticks: 90,
            tick_interval: 15,
            refresh_on_apply: true,
        });
        self.status_reg.register(StatusDef {
            name: "hex".into(),
            max_stacks: 3,
            duration_ticks: 120,
            tick_interval: 0,
            refresh_on_apply: true,
        });
        self.status_reg.register(StatusDef {
            name: "discord".into(),
            max_stacks: 20,
            duration_ticks: 180,
            tick_interval: 30,
            refresh_on_apply: true,
        });
        self.dummy_entities.clear();
        for i in 0..64 {
            let a = i as f32 / 64.0 * std::f32::consts::TAU;
            let p = Vec2::new(a.cos(), a.sin()) * 8.0;
            let e = eng
                .world
                .spawn((Pos(p), PrevPos(p), Vel(Vec2::ZERO), StatusBag::new()));
            self.dummy_entities.push(e);
        }
        eng.camera.zoom = 18.0;
    }

    fn init_realm(&mut self, eng: &mut Engine, _gfx: Option<&mut Renderer>) {
        eng.world = World::new();
        eng.schedule = Schedule::new();
        eng.clock.tick = 0;
        self.templates = dummy_room_templates();
        let grammar = RealmGrammar {
            start: "hub".into(),
            target_rooms: 15,
            allow: vec![],
        };
        let mut rng = eng.streams.get("layout").clone();
        self.realm = assemble(&self.templates, &grammar, &mut rng);
        *eng.streams.get("layout") = rng;
        eng.camera.zoom = 28.0;
        eng.camera.target = Vec3::ZERO;
    }

    fn tick_horde(&mut self, eng: &mut Engine) {
        horde::tick(eng);
        // Ambient particle rain so the GPU particle path stays hot (~200k target).
        self.particle_pulse = self.particle_pulse.wrapping_add(1);
        self.feel_cd = self.feel_cd.saturating_sub(1);

        // LMB feel-shot handled in fixed_update via mouse edge stored on input.
        if eng.input.mouse_pressed(0) && self.feel_cd == 0 {
            self.feel_shot(eng);
            self.feel_cd = 8;
        }
    }

    fn feel_shot(&mut self, eng: &mut Engine) {
        let ground = eng.mouse_ground();
        let origin = Vec3::new(0.0, 1.2, 0.0);
        let dir = (ground - origin).normalize_or_zero();
        let flat = Vec2::new(dir.x, dir.z);
        let flat = if flat.length_squared() < 1e-6 {
            Vec2::new(1.0, 0.0)
        } else {
            flat.normalize()
        };

        // One juicy projectile from the turret toward the cursor.
        eng.world.spawn((
            Pos(flat * 0.9),
            PrevPos(flat * 0.9),
            Vel(flat * 22.0),
            Projectile {
                life: 180,
                radius: 0.25,
                glow: true,
                ..Default::default()
            },
        ));

        eng.hitstop(0.045);
        eng.shake(0.35);
        eng.floaters.spawn(
            origin + Vec3::Y * 0.5,
            "FEEL",
            Vec4::new(1.0, 0.85, 0.35, 1.0),
            2.5,
        );

        if let Some(s) = &self.sfx_fire {
            eng.audio
                .play(s, "sfx", 0.7, 0.95 + (eng.clock.tick % 7) as f32 * 0.02);
        }
    }

    fn tick_proc_chain(&mut self, eng: &mut Engine) {
        // Every 4 ticks, seed a kill that deliberately loops through the vocab.
        if eng.clock.tick.is_multiple_of(4) {
            let src = self
                .dummy_entities
                .get((eng.clock.tick as usize / 4) % self.dummy_entities.len().max(1))
                .copied()
                .unwrap_or(Entity::DEAD);
            eng.triggers.emit(TriggerKind::of("on_kill"), src, src, 4.0);
        }

        // Status lifecycle on a rotating subset.
        let mut events = Vec::new();
        let reg = &self.status_reg;
        for (i, &e) in self.dummy_entities.iter().enumerate() {
            if eng.clock.tick as usize % 16 == i % 16 {
                if let Some(bag) = eng.world.get_mut::<StatusBag>(e) {
                    if let Some(def) = reg.get(StatusId::of("ignite")) {
                        bag.apply(e, def, 1, 2.0, &mut events);
                    }
                }
            }
            if let Some(bag) = eng.world.get_mut::<StatusBag>(e) {
                bag.tick(e, reg, &mut events);
            }
        }
        // Status events fan into the trigger bus.
        for ev in events {
            match ev {
                StatusEvent::Applied { entity, .. } => {
                    eng.triggers
                        .emit(TriggerKind::of("on_status_apply"), entity, entity, 1.0);
                }
                StatusEvent::Ticked {
                    entity, magnitude, ..
                } => {
                    eng.triggers.emit(
                        TriggerKind::of("on_status_detonate"),
                        entity,
                        entity,
                        magnitude,
                    );
                }
                StatusEvent::Detonated {
                    entity, magnitude, ..
                } => {
                    eng.triggers.emit(
                        TriggerKind::of("on_status_detonate"),
                        entity,
                        entity,
                        magnitude * 2.0,
                    );
                }
                StatusEvent::Expired { entity, .. } => {
                    eng.triggers
                        .emit(TriggerKind::of("on_crit"), entity, entity, 0.5);
                }
            }
        }

        // Infinite-loop vocabulary: on_kill → on_status_apply → on_crit → on_kill ×2
        eng.triggers.process(|ev, em| {
            let next = if ev.kind == TriggerKind::of("on_kill") {
                TriggerKind::of("on_status_apply")
            } else if ev.kind == TriggerKind::of("on_status_apply") {
                TriggerKind::of("on_crit")
            } else {
                // on_crit (and anything else) closes the cycle back to on_kill.
                TriggerKind::of("on_kill")
            };
            em.emit(next, ev.source, ev.target, ev.magnitude);
            em.emit(next, ev.source, ev.target, ev.magnitude);
        });

        // Orbit the dummies so the scene isn't static.
        let t = eng.clock.tick as f32 * FIXED_DT;
        eng.world.each::<(&mut Pos, &mut PrevPos)>(|_, (p, pp)| {
            pp.0 = p.0;
            let a = p.0.y.atan2(p.0.x) + 0.01;
            let r = p.0.length().max(0.1);
            p.0 = Vec2::new(a.cos(), a.sin()) * r;
            let _ = t;
        });
    }

    fn tick_realm(&mut self, eng: &mut Engine) {
        // Slow camera orbit over the assembled realm.
        let t = eng.clock.tick as f32 * FIXED_DT * 0.15;
        let r = 18.0;
        eng.camera.target = Vec3::new(t.cos() * r, 0.0, t.sin() * r * 0.7);
    }

    fn extract_horde(&mut self, eng: &mut Engine, frame: &mut FrameSubmit, alpha: f32) {
        let Some(mesh_enemy) = self.mesh_enemy else {
            return;
        };
        let Some(mesh_proj) = self.mesh_proj else {
            return;
        };
        let Some(mesh_corpse) = self.mesh_corpse else {
            return;
        };
        let Some(mesh_ground) = self.mesh_ground else {
            return;
        };
        let Some(mesh_turret) = self.mesh_turret else {
            return;
        };

        // Ground
        frame.meshes.push((
            mesh_ground,
            vec![InstanceRaw::new(
                Mat4::from_translation(Vec3::new(0.0, -0.05, 0.0)),
                Vec4::new(0.08, 0.06, 0.10, 1.0),
            )],
        ));

        // Turret at origin
        frame.meshes.push((
            mesh_turret,
            vec![InstanceRaw::new(
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(1.0),
                    Quat::IDENTITY,
                    Vec3::new(0.0, 0.0, 0.0),
                ),
                Vec4::new(0.55, 0.5, 0.45, 1.0),
            )
            .emissive(palette::GOLD, 1.4)
            .wobble(0.02, 2.0)],
        ));

        // Enemies
        let mut enemy_inst = Vec::with_capacity(horde::ENEMY_COUNT);
        eng.world.each::<(&Pos, &PrevPos, &Enemy)>(|_, (p, pp, e)| {
            let pos = pp.0.lerp(p.0, alpha);
            let telegraph = matches!(e.state, AiState::Telegraph);
            let lunge = matches!(e.state, AiState::Lunge);
            let color = if telegraph {
                Vec4::new(1.0, 0.25, 0.1, 1.0)
            } else if lunge {
                Vec4::new(1.0, 0.55, 0.15, 1.0)
            } else {
                Vec4::new(0.45, 0.18, 0.55, 1.0)
            };
            let em = if telegraph {
                palette::BRIMSTONE
            } else {
                palette::HEX
            };
            let s = if lunge { 1.15 } else { 1.0 };
            enemy_inst.push(
                InstanceRaw::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(s),
                        Quat::IDENTITY,
                        Vec3::new(pos.x, 0.0, pos.y),
                    ),
                    color,
                )
                .emissive(em, if telegraph { 2.5 } else { 0.6 })
                .phase(e.phase)
                .wobble(0.06, 3.0 + e.phase.fract() * 2.0),
            );
        });
        frame.meshes.push((mesh_enemy, enemy_inst));

        // Projectiles + lights
        let mut proj_inst = Vec::with_capacity(3200);
        eng.world
            .each::<(&Pos, &PrevPos, &Projectile)>(|_, (p, pp, pr)| {
                let pos = pp.0.lerp(p.0, alpha);
                let world = Vec3::new(pos.x, 0.5, pos.y);
                proj_inst.push(
                    InstanceRaw::new(
                        Mat4::from_scale_rotation_translation(
                            Vec3::splat(0.35),
                            Quat::IDENTITY,
                            world,
                        ),
                        Vec4::new(1.0, 0.7, 0.3, 1.0),
                    )
                    .emissive(palette::BRIMSTONE, 3.0),
                );
                if pr.glow || proj_inst.len() % 60 == 0 {
                    frame.lights.push(Light {
                        pos: world,
                        color: palette::BRIMSTONE,
                        radius: 4.5,
                        intensity: 2.2,
                    });
                }
            });
        // Keep ~50 lights even when glow is sparse.
        if frame.lights.len() < 50 {
            let need = 50 - frame.lights.len();
            let step = (proj_inst.len() / need.max(1)).max(1);
            for (_i, inst) in proj_inst.iter().enumerate().step_by(step) {
                if frame.lights.len() >= 50 {
                    break;
                }
                let t = inst.model[3];
                frame.lights.push(Light {
                    pos: Vec3::new(t[0], t[1], t[2]),
                    color: palette::HEX,
                    radius: 3.0,
                    intensity: 1.4,
                });
            }
        }
        frame.meshes.push((mesh_proj, proj_inst));

        // Corpses
        let mut corpse_inst = Vec::new();
        eng.world.each::<(&Pos, &Corpse)>(|_, (p, c)| {
            let t = c.age as f32 / c.max_age as f32;
            corpse_inst.push(
                InstanceRaw::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(1.0, 0.3, 1.0),
                        Quat::IDENTITY,
                        Vec3::new(p.0.x, 0.0, p.0.y),
                    ),
                    Vec4::new(0.2, 0.08, 0.1, 1.0 - t * 0.7),
                )
                .dissolve(t),
            );
        });
        frame.meshes.push((mesh_corpse, corpse_inst));

        // Drain impact FX → particles + floaters + shake
        let impacts: Vec<_> = {
            let fx = eng.world.resource_mut::<FxQueue>();
            fx.impacts.drain(..).collect()
        };
        for (pos, strength) in impacts {
            let world = Vec3::new(pos.x, 0.4, pos.y);
            let count = (40.0 * strength) as u32;
            frame.particle_spawns.push(ParticleSpawn {
                pos: world,
                count,
                vel: Vec3::Y * 2.0,
                spread: 5.0 * strength,
                color_from: Vec4::new(1.0, 0.7, 0.2, 1.0),
                color_to: Vec4::new(0.8, 0.1, 0.05, 0.0),
                size: (0.05, 0.16),
                life: (0.25, 0.7),
                gravity: 8.0,
                drag: 1.2,
            });
            if strength >= 3.0 {
                eng.floaters
                    .spawn(world + Vec3::Y, "KILL", Vec4::new(1.0, 0.4, 0.15, 1.0), 2.0);
                eng.shake(0.12);
                if let Some(s) = &self.sfx_hit {
                    eng.audio.play(s, "sfx", 0.45, 0.9);
                }
            } else {
                eng.floaters.spawn(
                    world + Vec3::Y * 0.5,
                    "5",
                    Vec4::new(1.0, 0.9, 0.6, 1.0),
                    1.4,
                );
            }
        }

        // Steady particle ambient (~3k/frame → ~200k live at 0.6s life)
        if eng.clock.tick.is_multiple_of(2) {
            frame.particle_spawns.push(ParticleSpawn {
                pos: Vec3::new(0.0, 0.5, 0.0),
                count: 2800,
                vel: Vec3::ZERO,
                spread: 22.0,
                color_from: Vec4::new(0.4, 0.15, 0.7, 0.5),
                color_to: Vec4::new(0.1, 0.02, 0.2, 0.0),
                size: (0.03, 0.08),
                life: (0.4, 0.9),
                gravity: 1.0,
                drag: 0.4,
            });
        }

        frame.ambient = Vec3::new(0.08, 0.07, 0.11);
        frame.fog = Vec4::new(0.03, 0.02, 0.05, 0.012);
        frame.bloom = 1.15;

        // Center light
        frame.lights.push(Light {
            pos: Vec3::new(0.0, 3.0, 0.0),
            color: palette::BONE,
            radius: 18.0,
            intensity: 1.8,
        });
    }

    fn extract_proc_chain(&mut self, eng: &mut Engine, frame: &mut FrameSubmit, alpha: f32) {
        let Some(mesh_enemy) = self.mesh_enemy else {
            return;
        };
        let Some(mesh_ground) = self.mesh_ground else {
            return;
        };

        frame.meshes.push((
            mesh_ground,
            vec![InstanceRaw::new(
                Mat4::from_translation(Vec3::new(0.0, -0.05, 0.0)),
                Vec4::new(0.06, 0.05, 0.09, 1.0),
            )],
        ));

        let mut inst = Vec::new();
        eng.world
            .each::<(&Pos, &PrevPos, &StatusBag)>(|_, (p, pp, bag)| {
                let pos = pp.0.lerp(p.0, alpha);
                let stacks = bag.stacks(StatusId::of("ignite"));
                let heat = (stacks as f32 / 8.0).clamp(0.0, 1.0);
                inst.push(
                    InstanceRaw::new(
                        Mat4::from_translation(Vec3::new(pos.x, 0.0, pos.y)),
                        Vec4::new(0.3 + heat * 0.7, 0.15, 0.4, 1.0),
                    )
                    .emissive(palette::HEX, 0.5 + heat * 3.0)
                    .wobble(0.08, 4.0),
                );
                if stacks > 0 {
                    frame.lights.push(Light {
                        pos: Vec3::new(pos.x, 1.0, pos.y),
                        color: palette::HEX,
                        radius: 3.0 + heat * 2.0,
                        intensity: 1.0 + heat,
                    });
                }
            });
        frame.meshes.push((mesh_enemy, inst));

        // Visualize budget pressure with a particle burst when budget hits.
        if eng.triggers.stats.budget_hit_ticks > 0
            && eng.triggers.stats.last_tick_processed > 0
            && eng.clock.tick.is_multiple_of(8)
        {
            frame.particle_spawns.push(ParticleSpawn {
                pos: Vec3::Y * 1.5,
                count: 400,
                vel: Vec3::Y * 3.0,
                spread: 8.0,
                color_from: Vec4::new(0.9, 0.2, 1.0, 1.0),
                color_to: Vec4::new(0.2, 0.0, 0.4, 0.0),
                size: (0.04, 0.12),
                life: (0.3, 0.8),
                gravity: 4.0,
                drag: 1.0,
            });
        }

        frame.ambient = Vec3::new(0.07, 0.05, 0.12);
        frame.bloom = 1.3;
        frame.lights.push(Light {
            pos: Vec3::new(0.0, 4.0, 0.0),
            color: palette::HEX,
            radius: 20.0,
            intensity: 2.0,
        });
    }

    fn extract_realm(&mut self, eng: &mut Engine, frame: &mut FrameSubmit, _alpha: f32) {
        let Some(mesh_room) = self.mesh_room else {
            return;
        };
        let Some(mesh_column) = self.mesh_column else {
            return;
        };
        let Some(layout) = &self.realm else { return };

        const CELL: f32 = 3.5;
        let mut rooms = Vec::new();
        let mut columns = Vec::new();

        for (i, r) in layout.rooms.iter().enumerate() {
            let t = &self.templates[r.template];
            let cx = (r.x as f32 + t.width as f32 * 0.5) * CELL;
            let cz = (r.y as f32 + t.height as f32 * 0.5) * CELL;
            let sx = t.width as f32 * CELL * 0.95;
            let sz = t.height as f32 * CELL * 0.95;
            let is_start = i == 0;
            let color = if is_start {
                Vec4::new(0.25, 0.18, 0.12, 1.0)
            } else {
                Vec4::new(0.12, 0.10, 0.16, 1.0)
            };
            rooms.push(
                InstanceRaw::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(sx, 0.2, sz),
                        Quat::IDENTITY,
                        Vec3::new(cx, 0.0, cz),
                    ),
                    color,
                )
                .emissive(
                    if is_start {
                        palette::GOLD
                    } else {
                        palette::ASH
                    },
                    if is_start { 0.8 } else { 0.15 },
                ),
            );
            // Corner columns
            for (dx, dz) in [(-0.4, -0.4), (0.4, -0.4), (-0.4, 0.4), (0.4, 0.4)] {
                columns.push(
                    InstanceRaw::new(
                        Mat4::from_scale_rotation_translation(
                            Vec3::new(0.35, 1.0, 0.35),
                            Quat::IDENTITY,
                            Vec3::new(cx + dx * sx, 0.0, cz + dz * sz),
                        ),
                        Vec4::new(0.2, 0.18, 0.22, 1.0),
                    )
                    .emissive(palette::BONE, 0.25)
                    .wobble(0.01, 1.0),
                );
            }
            // Soft room light
            frame.lights.push(Light {
                pos: Vec3::new(cx, 2.5, cz),
                color: if is_start {
                    palette::GOLD
                } else if i % 3 == 0 {
                    palette::HEX
                } else {
                    palette::BONE
                },
                radius: 8.0 + t.width as f32,
                intensity: if is_start { 2.5 } else { 1.2 },
            });
        }

        // Connection markers
        for &(_, _, (x, y)) in &layout.connections {
            let wx = x as f32 * CELL;
            let wz = y as f32 * CELL;
            frame.particle_spawns.push(ParticleSpawn {
                pos: Vec3::new(wx, 0.5, wz),
                count: 4,
                vel: Vec3::Y * 0.5,
                spread: 0.4,
                color_from: Vec4::new(0.6, 0.5, 0.9, 0.6),
                color_to: Vec4::new(0.2, 0.1, 0.4, 0.0),
                size: (0.05, 0.1),
                life: (0.5, 1.0),
                gravity: -0.5,
                drag: 0.5,
            });
        }

        frame.meshes.push((mesh_room, rooms));
        frame.meshes.push((mesh_column, columns));
        frame.ambient = Vec3::new(0.06, 0.05, 0.09);
        frame.fog = Vec4::new(0.02, 0.015, 0.04, 0.025);
        frame.bloom = 1.0;

        let _ = eng;
    }
}

impl Game for SandboxGame {
    fn init(&mut self, eng: &mut Engine, gfx: Option<&mut Renderer>) {
        if let Some(r) = gfx {
            self.mesh_enemy = Some(
                r.register_mesh(
                    MeshBuilder::prism(5, 0.45, 1.1)
                        .tapered(0.55)
                        .jittered(0.04)
                        .merged(MeshBuilder::spike(0.7).translated(Vec3::Y * 1.0)),
                ),
            );
            self.mesh_proj = Some(r.register_mesh(MeshBuilder::orb(2, 0.5)));
            self.mesh_corpse =
                Some(r.register_mesh(MeshBuilder::prism(4, 0.5, 0.35).jittered(0.08)));
            self.mesh_ground = Some(r.register_mesh(MeshBuilder::ground(60.0, 60.0)));
            self.mesh_turret = Some(
                r.register_mesh(
                    MeshBuilder::column(1.6)
                        .twisted(0.15)
                        .merged(MeshBuilder::orb(3, 0.55).translated(Vec3::Y * 1.7)),
                ),
            );
            self.mesh_room = Some(r.register_mesh(MeshBuilder::cube()));
            self.mesh_column =
                Some(r.register_mesh(MeshBuilder::column(2.4).twisted(0.08).obsidian_ish()));
        }

        self.sfx_fire = Some(Sound::blip(440.0, 0.06));
        self.sfx_hit = Some(Sound::noise_burst(0.08, 180.0));
        eng.audio.set_bus("sfx", 0.8);
        eng.audio.set_bus("music", 0.5);

        self.switch(eng, None, self.mode);
    }

    fn fixed_update(&mut self, eng: &mut Engine) {
        // Mode hotkeys
        if eng.input.key_pressed(KeyCode::Digit1) {
            self.switch(eng, None, SceneMode::Horde);
        } else if eng.input.key_pressed(KeyCode::Digit2) {
            self.switch(eng, None, SceneMode::ProcChain);
        } else if eng.input.key_pressed(KeyCode::Digit3) {
            self.switch(eng, None, SceneMode::Realm);
        }
        if eng.input.key_pressed(KeyCode::Escape) {
            eng.quit = true;
        }
        if eng.input.scroll.abs() > 0.0 {
            eng.camera.zoom_by(1.0 - eng.input.scroll * 0.08);
        }

        match self.mode {
            SceneMode::Horde => self.tick_horde(eng),
            SceneMode::ProcChain => self.tick_proc_chain(eng),
            SceneMode::Realm => self.tick_realm(eng),
        }
    }

    fn render_extract(&mut self, eng: &mut Engine, frame: &mut FrameSubmit, alpha: f32) {
        match self.mode {
            SceneMode::Horde => self.extract_horde(eng, frame, alpha),
            SceneMode::ProcChain => self.extract_proc_chain(eng, frame, alpha),
            SceneMode::Realm => self.extract_realm(eng, frame, alpha),
        }

        // Overlay lines
        eng.overlay.lines.clear();
        let mode_name = match self.mode {
            SceneMode::Horde => "HORDE",
            SceneMode::ProcChain => "PROC-CHAIN",
            SceneMode::Realm => "REALM",
        };
        eng.overlay
            .lines
            .push(format!("scene: {mode_name}  [1/2/3]"));
        match self.mode {
            SceneMode::Horde => {
                let mut n_e = 0usize;
                let mut n_p = 0usize;
                let mut n_c = 0usize;
                eng.world.each::<(&Enemy,)>(|_, _| n_e += 1);
                eng.world.each::<(&Projectile,)>(|_, _| n_p += 1);
                eng.world.each::<(&Corpse,)>(|_, _| n_c += 1);
                eng.overlay.lines.push(format!(
                    "enemies {n_e}  projectiles {n_p}  corpses {n_c}  fire/tick {FIRE_PER_TICK}"
                ));
                eng.overlay
                    .lines
                    .push("LMB: feel-shot (hitstop+shake+bloom+sfx)".into());
            }
            SceneMode::ProcChain => {
                let s = eng.triggers.stats;
                eng.overlay.lines.push(format!(
                    "triggers processed {}  dropped_budget {}  budget_hits {}",
                    s.processed, s.dropped_budget, s.budget_hit_ticks
                ));
                eng.overlay.lines.push(format!(
                    "max_depth {}  last_tick {}  pending {}",
                    s.max_depth_seen,
                    s.last_tick_processed,
                    eng.triggers.pending()
                ));
            }
            SceneMode::Realm => {
                if let Some(l) = &self.realm {
                    eng.overlay.lines.push(format!(
                        "rooms {}  connections {}  hash {:#x}",
                        l.rooms.len(),
                        l.connections.len(),
                        l.hash()
                    ));
                }
            }
        }
    }

    fn ui(&mut self, eng: &mut Engine, ui: &mut UiBatch) {
        let label = match self.mode {
            SceneMode::Horde => "GOETIA  /  HORDE STRESS",
            SceneMode::ProcChain => "GOETIA  /  PROC CHAIN",
            SceneMode::Realm => "GOETIA  /  REALM GRAMMAR",
        };
        ui.text_shadowed(
            Vec2::new(eng.viewport.x * 0.5 - 140.0, eng.viewport.y - 28.0),
            2.0,
            Vec4::new(0.9, 0.85, 0.7, 0.9),
            label,
        );
    }
}

// ---------------------------------------------------------------- helpers

fn dummy_room_templates() -> Vec<RoomTemplate> {
    fn t(name: &str, w: u32, h: u32, doors: Vec<Door>, tags: &[&str], weight: f32) -> RoomTemplate {
        RoomTemplate {
            name: name.into(),
            width: w,
            height: h,
            doors,
            tags: tags.iter().map(|s| (*s).into()).collect(),
            weight,
        }
    }
    vec![
        t(
            "hub",
            4,
            4,
            vec![
                Door {
                    side: Side::North,
                    offset: 1,
                },
                Door {
                    side: Side::South,
                    offset: 2,
                },
                Door {
                    side: Side::East,
                    offset: 1,
                },
                Door {
                    side: Side::West,
                    offset: 2,
                },
            ],
            &["hub"],
            1.0,
        ),
        t(
            "hall_ns",
            2,
            5,
            vec![
                Door {
                    side: Side::North,
                    offset: 0,
                },
                Door {
                    side: Side::South,
                    offset: 1,
                },
            ],
            &["corridor"],
            1.4,
        ),
        t(
            "hall_ew",
            5,
            2,
            vec![
                Door {
                    side: Side::East,
                    offset: 0,
                },
                Door {
                    side: Side::West,
                    offset: 1,
                },
            ],
            &["corridor"],
            1.4,
        ),
        t(
            "chamber",
            3,
            3,
            vec![
                Door {
                    side: Side::North,
                    offset: 1,
                },
                Door {
                    side: Side::West,
                    offset: 1,
                },
            ],
            &["combat"],
            1.0,
        ),
        t(
            "vault",
            3,
            2,
            vec![Door {
                side: Side::South,
                offset: 1,
            }],
            &["loot"],
            0.7,
        ),
        t(
            "cross",
            3,
            3,
            vec![
                Door {
                    side: Side::North,
                    offset: 1,
                },
                Door {
                    side: Side::South,
                    offset: 1,
                },
                Door {
                    side: Side::East,
                    offset: 1,
                },
                Door {
                    side: Side::West,
                    offset: 1,
                },
            ],
            &["hub", "combat"],
            1.1,
        ),
    ]
}

/// Cosmetic alias so the mesh authoring API reads like the contract examples.
trait MeshStyle {
    fn obsidian_ish(self) -> Self;
}
impl MeshStyle for MeshBuilder {
    fn obsidian_ish(self) -> Self {
        self.jittered(0.03)
    }
}
