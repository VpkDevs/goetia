//! The game state machine: Court ↔ Run ↔ Dead. Owns Gs, meshes, menus,
//! progress, and the save cadence.

use crate::combat::*;
use crate::content::ContentDb;
use crate::court::{enter_court, tick_court, CourtState};
use crate::fx::Sounds;
use crate::items::{compile_build, Loadout, LootTables};
use crate::render::{draw_court, draw_run, register_meshes, Meshes};
use crate::run::{generate, tick_run, RunEvent, RunState};
use crate::save::{load_all, save_all, Progress};
use crate::ui::{draw_death, draw_hud_court, draw_hud_run, draw_menu, menu_input, MenuState};
use crate::vocab::DEMONS;
use goetia::prelude::*;

enum Mode {
    Court,
    Run(RunState),
    Dead { timer: u16, lost: usize },
}

pub struct Demonicon {
    gs: Option<Gs>,
    meshes: Option<Meshes>,
    mode: Mode,
    court: CourtState,
    menu: MenuState,
    progress: Progress,
    /// Debug hot-reload poll cadence.
    reload_timer: u32,
    /// Debug: descend immediately on launch (--demon N --tier N).
    pub autostart: Option<(usize, u32)>,
}

impl Demonicon {
    pub fn new() -> Demonicon {
        Demonicon {
            gs: None,
            meshes: None,
            mode: Mode::Court,
            court: CourtState::default(),
            menu: MenuState::default(),
            progress: Progress::default(),
            reload_timer: 0,
            autostart: None,
        }
    }

    fn default_loadout(db: &ContentDb) -> Loadout {
        let mut l = Loadout::new();
        // A working starter kit: bolt + nova + dash. Everything else is yours
        // to discover — respec is free from minute one.
        l.skills[0] = Some("hellbolt".into());
        l.skills[1] = Some("voidnova".into());
        l.skills[2] = Some("ripdash".into());
        let _ = db;
        l
    }
}

impl Default for Demonicon {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for Demonicon {
    fn init(&mut self, eng: &mut Engine, gfx: Option<&mut Renderer>) {
        let db = ContentDb::load_all();
        let errs = crate::content::validate(&db);
        for e in &errs {
            log::warn!("content: {e}");
        }
        let status_reg = db.build_status_registry();
        let loaded = load_all();
        let (loadout, bank, dust, loot, progress) = match loaded {
            Some(l) => (l.loadout, l.bank, l.dust, l.loot, l.progress),
            None => (Self::default_loadout(&db), Vec::new(), 30, LootTables::default(), Progress::default()),
        };
        self.progress = progress;
        let build = compile_build(&db, &loadout);
        let mut gs = Gs {
            db,
            status_reg,
            loadout,
            build,
            bank,
            run_inv: Vec::new(),
            dust,
            loot,
            sounds: Sounds::synth(),
            pc: PlayerCtx::new(Entity::DEAD),
            loot_mul: 1.0,
            reveal_on_drop: false,
            death_novas: false,
            boss_reflect: Vec::new(),
            last_player_trigger: None,
            blight_phase: false,
            tier: 1,
            walkable: None,
        };
        if let Some(gfx) = gfx {
            self.meshes = Some(register_meshes(gfx));
        }
        eng.audio.set_bus("sfx", 0.85);
        // The proc engine's referee: generous budget, no depth cap worth
        // noticing, geometric falloff. Loops are content.
        eng.triggers.config = TriggerConfig {
            budget_per_tick: 3000,
            max_chain_depth: 64,
            chain_falloff: 0.85,
            magnitude_floor: 0.0,
        };
        enter_court(eng, &mut gs);
        self.gs = Some(gs);
    }

    fn fixed_update(&mut self, eng: &mut Engine) {
        let gs = self.gs.as_mut().unwrap();

        // Hot reload (debug): tune an affix without recompiling.
        self.reload_timer += 1;
        if self.reload_timer >= 60 {
            self.reload_timer = 0;
            if gs.db.poll() {
                gs.status_reg = gs.db.build_status_registry();
                gs.recompile();
                eng.overlay.lines.push("CONTENT RELOADED".into());
            }
        }

        if eng.input.key_pressed(KeyCode::Escape) {
            if self.menu.open {
                self.menu.open = false;
            } else {
                save_all(gs, &self.progress);
                eng.quit = true;
            }
            return;
        }

        if let Some((d, t)) = self.autostart.take() {
            self.court.sel_demon = d.min(2);
            self.court.tier = t.max(1);
            let rs = generate(eng, gs, DEMONS[self.court.sel_demon], self.court.tier);
            self.mode = Mode::Run(rs);
            return;
        }

        match &mut self.mode {
            Mode::Court => {
                let changed = menu_input(eng, gs, &mut self.menu, false);
                if changed {
                    save_all(gs, &self.progress);
                }
                if !self.menu.open {
                    if let Some((demon, tier)) = tick_court(eng, gs, &mut self.court) {
                        self.menu.open = false;
                        let rs = generate(eng, gs, demon, tier);
                        // Announce the realm's terms.
                        for m in &rs.mods {
                            eng.floaters.spawn(
                                Vec3::new(rs.entry.x, 2.0, rs.entry.y),
                                m.text.clone(),
                                palette::GOLD.extend(1.0),
                                1.6,
                            );
                        }
                        self.mode = Mode::Run(rs);
                    }
                }
            }
            Mode::Run(rs) => {
                let changed = menu_input(eng, gs, &mut self.menu, true);
                if changed {
                    // Equipment swaps mid-run are legal; persist lazily on exit.
                }
                if self.menu.open {
                    return; // menu pauses the run (solo game, honest pause)
                }
                match tick_run(eng, gs, rs) {
                    RunEvent::PlayerDied => {
                        let lost = gs.run_inv.len();
                        gs.run_inv.clear();
                        gs.walkable = None;
                        self.progress.deaths += 1;
                        save_all(gs, &self.progress);
                        enter_court(eng, gs); // clear the realm behind the shroud
                        self.mode = Mode::Dead { timer: 240, lost };
                    }
                    RunEvent::ReturnedToCourt | RunEvent::None => {
                        // Sentinel from the portal interact: bank and go home.
                        if rs.ticks == u64::MAX {
                            let banked = gs.run_inv.len();
                            let cleared = rs.cleared;
                            let demon = rs.demon;
                            let items: Vec<_> = gs.run_inv.drain(..).collect();
                            gs.bank.extend(items);
                            gs.walkable = None;
                            self.progress.runs += 1;
                            if cleared {
                                let d = demon.index();
                                self.progress.cleared[d] = self.progress.cleared[d].max(gs.tier);
                                // Next tier unlock nudge.
                                self.court.tier = gs.tier + 1;
                            }
                            save_all(gs, &self.progress);
                            enter_court(eng, gs);
                            eng.floaters.spawn(
                                Vec3::new(0.0, 2.0, 0.0),
                                format!("{banked} SPOILS BANKED"),
                                palette::GOLD.extend(1.0),
                                2.0,
                            );
                            self.mode = Mode::Court;
                        }
                    }
                }
            }
            Mode::Dead { timer, .. } => {
                *timer = timer.saturating_sub(1);
                if *timer == 0 {
                    enter_court(eng, gs);
                    self.mode = Mode::Court;
                }
            }
        }
    }

    fn render_extract(&mut self, eng: &mut Engine, frame: &mut FrameSubmit, alpha: f32) {
        let gs = self.gs.as_mut().unwrap();
        let Some(m) = self.meshes.as_ref() else { return };
        match &self.mode {
            Mode::Court => {
                draw_court(eng, gs, m, frame, alpha, self.court.sel_demon);
                draw_hud_court(eng, gs, &mut frame.ui, self.court.sel_demon, self.court.tier, &self.progress);
            }
            Mode::Run(rs) => {
                draw_run(eng, gs, rs, m, frame, alpha);
                draw_hud_run(eng, gs, rs, &mut frame.ui);
                // Trigger-engine pulse readout: build tinkerers live off this.
                let t = eng.triggers.stats;
                if t.last_tick_processed > 40 {
                    eng.overlay.lines.push(format!(
                        "PROC ENGINE: {}/TICK DEPTH {}",
                        t.last_tick_processed, t.max_depth_seen
                    ));
                }
            }
            Mode::Dead { timer, lost } => {
                // Keep drawing the world you died in, dimmed by the shroud.
                if let Mode::Dead { .. } = self.mode {}
                draw_court(eng, gs, m, frame, alpha, self.court.sel_demon);
                draw_death(eng, &mut frame.ui, *timer, *lost);
            }
        }
        draw_menu(eng, gs, &self.menu, &mut frame.ui, matches!(self.mode, Mode::Run(_)));
    }
}
