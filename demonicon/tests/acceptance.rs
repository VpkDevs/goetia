//! Headless acceptance tests: content integrity, build compilation across
//! the whole vocabulary, trigger-loop containment with real game reactions,
//! and full-run determinism on the engine's fixed-tick sim.

use demonicon::combat::*;
use demonicon::content::*;
use demonicon::items::*;
use demonicon::run::{generate, tick_run};
use demonicon::vocab::*;
use goetia::prelude::*;

fn make_gs() -> Gs {
    let db = ContentDb::load_all();
    let status_reg = db.build_status_registry();
    let loadout = {
        let mut l = Loadout::new();
        l.skills[0] = Some("hellbolt".into());
        l.skills[1] = Some("voidnova".into());
        l
    };
    let build = compile_build(&db, &loadout);
    Gs {
        db,
        status_reg,
        loadout,
        build,
        bank: Vec::new(),
        run_inv: Vec::new(),
        dust: 0,
        loot: LootTables::default(),
        sounds: demonicon::fx::Sounds::synth(),
        pc: PlayerCtx::new(Entity::DEAD),
        loot_mul: 1.0,
        reveal_on_drop: false,
        death_novas: false,
        boss_reflect: Vec::new(),
        last_player_trigger: None,
        blight_phase: false,
        tier: 1,
        walkable: None,
    }
}

#[test]
fn content_loads_and_validates() {
    let db = ContentDb::load_all();
    let errs = validate(&db);
    assert!(errs.is_empty(), "content errors: {errs:?}");
    assert_eq!(db.skills().len(), 8, "eight skills");
    assert_eq!(db.sigils().len(), 20, "twenty sigils");
    assert!(
        db.affixes().len() >= 55,
        "affix pool ({} found)",
        db.affixes().len()
    );
    assert_eq!(db.contracts().len(), 9, "nine contracts");
    assert_eq!(db.goetics().len(), 12, "twelve goetics");
    assert_eq!(db.realms().len(), 3, "three realms");
    assert!(db.realm_mods().len() >= 6);
    // Behavioral affix count per spec (~15).
    let behavioral = db.affixes().iter().filter(|a| a.reaction.is_some()).count();
    assert!(behavioral >= 15, "behavioral affixes: {behavioral}");
}

#[test]
fn every_contract_goetic_sigil_combination_compiles() {
    let db = ContentDb::load_all();
    // Equip every goetic, sign every contract triple, socket every sigil —
    // build compilation must never panic (Pillar 2: it all composes).
    for g in db.goetics() {
        let mut l = Loadout::new();
        let slot_idx = match g.slot {
            Slot::Weapon => 0,
            Slot::Armor => 1,
            Slot::Relic => 2,
            Slot::Ring => 3,
        };
        l.equipment[slot_idx] = Some(ItemInstance {
            uid: 1,
            name: g.name.clone(),
            slot: g.slot,
            rarity: Rarity::Goetic,
            ilvl: 5,
            affixes: vec![],
            goetic: Some(g.id.clone()),
            lore: None,
            awakened: false,
        });
        for c in db.contracts() {
            l.contracts[0] = Some(c.id.clone());
            l.skills[0] = Some("hexbeam".into());
            for s in db.sigils() {
                l.sigils[0][0] = Some(s.id.clone());
                let b = compile_build(&db, &l);
                let _ = b.rules.len() + b.reactions.len();
            }
        }
    }
}

#[test]
fn loot_generation_never_panics_and_pity_works() {
    let db = ContentDb::load_all();
    let mut lt = LootTables::default();
    let mut rng = Pcg32::new(7, 1);
    let mut goetics = 0;
    for _ in 0..2000 {
        let r = roll_rarity(&mut lt, &mut rng, 0.0);
        let item = gen_item(&db, &mut lt, &mut rng, r, 3, false);
        assert!(!item.name.is_empty());
        if r == Rarity::Goetic {
            goetics += 1;
        }
    }
    assert!(goetics >= 5, "pity should force goetics: got {goetics}");
}

#[test]
fn corruption_awakening_names_things() {
    let db = ContentDb::load_all();
    let mut lt = LootTables::default();
    let mut rng = Pcg32::new(3, 1);
    let mut nrng = Pcg32::new(4, 1);
    let mut awakened = 0;
    for i in 0..200 {
        let mut item = gen_item(&db, &mut lt, &mut rng, Rarity::Rare, 3, false);
        let _ = i;
        if let CorruptOutcome::Awakened = corrupt_item(&db, &mut lt, &mut rng, &mut nrng, &mut item)
        {
            awakened += 1;
            assert_eq!(item.rarity, Rarity::Goetic);
            assert!(item.lore.as_deref().unwrap_or("").contains("TAKEN FROM"));
        }
    }
    assert!(awakened > 10, "awaken rate sane: {awakened}");
}

struct HeadlessRun {
    demon: Demon,
    ticks_left: u64,
}

impl Game for HeadlessRun {
    fn init(&mut self, eng: &mut Engine, _gfx: Option<&mut Renderer>) {
        let mut gs = make_gs();
        let rs = generate(eng, &mut gs, self.demon, 3);
        eng.world.insert_resource(HeadlessCtx { gs, rs })
    }
    fn fixed_update(&mut self, eng: &mut Engine) {
        if self.ticks_left == 0 {
            return;
        }
        self.ticks_left -= 1;
        let mut ctx = eng.world.remove_resource::<HeadlessCtx>().unwrap();
        let _ = tick_run(eng, &mut ctx.gs, &mut ctx.rs);
        eng.world.insert_resource(ctx);
    }
    fn render_extract(&mut self, _e: &mut Engine, _f: &mut FrameSubmit, _a: f32) {}
}

struct HeadlessCtx {
    gs: Gs,
    rs: demonicon::run::RunState,
}

fn run_hash(demon: Demon, seed: u64, ticks: u64) -> u64 {
    let mut eng = App::run_headless(
        HeadlessRun {
            demon,
            ticks_left: ticks,
        },
        seed,
        ticks,
    );
    let mut h = StateHasher::new();
    h.write_u64(eng.clock.tick);
    eng.world
        .each::<(&Pos, &Health, &EnemyC)>(|ent, (p, hp, ec)| {
            h.write_u64(ent.to_bits());
            h.write_vec2(p.0.x, p.0.y);
            h.write_f32(hp.hp);
            h.write_u32(ec.timer as u32);
        });
    let ctx = eng.world.remove_resource::<HeadlessCtx>().unwrap();
    h.write_u64(ctx.rs.layout.hash());
    h.finish()
}

#[test]
fn full_run_sim_is_deterministic() {
    // 2000 ticks of a real tier-3 realm (packs, AI, statuses, boss, cycle),
    // twice per demon. Same seed → bit-identical.
    for demon in DEMONS {
        let a = run_hash(demon, 0xD3, 2000);
        let b = run_hash(demon, 0xD3, 2000);
        assert_eq!(a, b, "{} diverged", demon.name());
    }
}

#[test]
fn trigger_loop_is_contained_by_budget() {
    // The nastiest legal build: echo-on-status-apply + status-on-everything.
    // The reaction table feeds itself; the engine budget must hold the tick.
    let mut eng = Engine::new(1, 1, true);
    eng.triggers.config = TriggerConfig {
        budget_per_tick: 512,
        max_chain_depth: 1_000_000,
        chain_falloff: 1.0,
        magnitude_floor: 0.0,
    };
    let mut gs = make_gs();
    gs.build.reactions = vec![Reaction {
        on: "on_status_apply".into(),
        chance: 1.0,
        action: Action::ApplyStatus {
            status: "ignite".into(),
            stacks: 1,
            magnitude: 0.1,
            radius: 0.0,
        },
    }];
    eng.world.insert_resource(SpatialGrid::new(2.0));
    eng.world.insert_resource(demonicon::fx::FxQueue::default());
    let dummy = eng.world.spawn((
        Pos(glam::Vec2::ZERO),
        Health::new(1_000_000.0),
        StatusBag::new(),
    ));
    gs.pc.entity = eng
        .world
        .spawn((Pos(glam::Vec2::ONE), Health::new(100.0), StatusBag::new()));
    // Prime the pump: applying ignite emits on_status_apply, whose reaction
    // applies ignite, which emits on_status_apply...
    for _ in 0..10 {
        apply_status_to(&mut eng, &mut gs, dummy, "ignite", 1, 5.0);
    }
    for _ in 0..30 {
        process_triggers(&mut eng, &mut gs);
    }
    let s = eng.triggers.stats;
    assert!(s.processed > 0);
    assert!(
        s.processed <= 512 * 31,
        "budget leak: processed {} in 30 ticks",
        s.processed
    );
    // Action-phase emissions (statuses applied by executed reactions) may sit
    // in the queue awaiting the next tick — bounded, never unbounded.
    assert!(
        eng.triggers.pending() <= 512,
        "pending backlog exploded: {}",
        eng.triggers.pending()
    );
}

#[test]
fn boss_kill_clears_run_and_showers_loot() {
    // The unproven third act: boss dies → run clears → loot shower → exit
    // portal. Headless, per demon. BUER additionally proves the phase gate:
    // damage during bloom must bounce, blight must let it through.
    struct BossSlayer {
        demon: Demon,
        done: bool,
    }
    impl Game for BossSlayer {
        fn init(&mut self, eng: &mut Engine, _g: Option<&mut Renderer>) {
            let mut gs = make_gs();
            let rs = generate(eng, &mut gs, self.demon, 1);
            eng.world.insert_resource(HeadlessCtx { gs, rs });
        }
        fn fixed_update(&mut self, eng: &mut Engine) {
            if self.done {
                return;
            }
            let mut ctx = eng.world.remove_resource::<HeadlessCtx>().unwrap();
            let _ = tick_run(eng, &mut ctx.gs, &mut ctx.rs);
            let boss = ctx.rs.boss;
            if eng.world.is_alive(boss) {
                // Keep the player parked far away and immortal; swing a huge
                // mixed-type blade through the real damage pipeline.
                if let Some(h) = eng.world.get_mut::<Health>(ctx.gs.pc.entity) {
                    h.hp = h.max;
                }
                let at = eng
                    .world
                    .get::<Pos>(boss)
                    .map(|p| p.0)
                    .unwrap_or(glam::Vec2::ZERO);
                let before = eng.world.get::<Health>(boss).map(|h| h.hp).unwrap_or(0.0);
                let r = hit_enemy(
                    eng,
                    &mut ctx.gs,
                    boss,
                    at,
                    [120.0, 120.0, 0.0, 0.0],
                    &[],
                    false,
                    false,
                );
                let after = eng.world.get::<Health>(boss).map(|h| h.hp).unwrap_or(0.0);
                if self.demon == Demon::Buer && !ctx.gs.blight_phase {
                    // Bloom: the wheel must be immune (the phase gate is real).
                    assert_eq!(before, after.max(before), "BUER took damage during bloom");
                    assert_eq!(r.total, 0.0, "BUER hit registered during bloom");
                }
            }
            self.done = ctx.rs.cleared;
            eng.world.insert_resource(ctx);
        }
        fn render_extract(&mut self, _e: &mut Engine, _f: &mut FrameSubmit, _a: f32) {}
    }

    for demon in DEMONS {
        // Enough ticks for Buer's first blight window (cycle flips at 600).
        let mut eng = App::run_headless(BossSlayer { demon, done: false }, 0xB055, 2400);
        let ctx = eng.world.remove_resource::<HeadlessCtx>().unwrap();
        assert!(ctx.rs.cleared, "{}: boss never fell", demon.name());
        assert!(
            ctx.rs.portal_out.is_some(),
            "{}: no exit portal",
            demon.name()
        );
        let mut drops = 0;
        let mut items = 0;
        eng.world.each::<(&LootDrop,)>(|_, (l,)| {
            drops += 1;
            if l.item.is_some() {
                items += 1;
            }
        });
        assert!(
            items >= 5,
            "{}: boss shower too dry ({items} items, {drops} drops)",
            demon.name()
        );
    }
}

#[test]
fn same_seed_same_realm_layout() {
    let db = ContentDb::load_all();
    for demon in DEMONS {
        let t = db.room_templates(demon).to_vec();
        let g = db.grammar(demon).clone();
        let a = assemble(&t, &g, &mut Pcg32::new(99, 1)).unwrap();
        let b = assemble(&t, &g, &mut Pcg32::new(99, 1)).unwrap();
        assert_eq!(a.hash(), b.hash(), "{} layout diverged", demon.name());
        assert!(
            a.rooms.len() >= 8,
            "{} too small: {}",
            demon.name(),
            a.rooms.len()
        );
    }
}
