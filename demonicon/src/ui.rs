//! HUD + the build/inventory menu. Keyboard-driven, bone-white on near-black,
//! every color meaningful (rarity, damage type, status). Free respec lives
//! here: no costs, no confirmation dialogs, no friction.

use crate::combat::*;
use crate::content::*;
use crate::items::*;
use crate::run::RunState;
use crate::vocab::*;
use goetia::prelude::*;

pub const WHITE: Vec4 = Vec4::new(0.85, 0.79, 0.65, 1.0);
pub const DIM: Vec4 = Vec4::new(0.45, 0.42, 0.5, 1.0);
pub const PANEL: Vec4 = Vec4::new(0.02, 0.015, 0.035, 0.88);

fn c4(v: Vec3) -> Vec4 {
    v.extend(1.0)
}

// -------------------------------------------------------------------- HUD

pub fn draw_hud_run(eng: &mut Engine, gs: &mut Gs, rs: &RunState, ui: &mut UiBatch) {
    let vp = eng.viewport;

    // Health bar (bottom left).
    let (hp, max) = eng
        .world
        .get::<Health>(gs.pc.entity)
        .map(|h| (h.hp, h.max))
        .unwrap_or((0.0, 1.0));
    let bx = 24.0;
    let by = vp.y - 88.0;
    ui.rect(Vec2::new(bx - 2.0, by - 2.0), Vec2::new(264.0, 30.0), PANEL);
    let frac = (hp / max).clamp(0.0, 1.0);
    let hp_color = if frac < 0.35 { c4(palette::BLOOD) } else { c4(palette::BONE * 0.9) };
    ui.rect(Vec2::new(bx, by), Vec2::new(260.0 * frac, 26.0), hp_color);
    ui.text_shadowed(Vec2::new(bx + 6.0, by + 6.0), 2.0, Vec4::new(0.05, 0.05, 0.08, 1.0), &format!("{:.0}/{:.0}", hp.max(0.0), max));

    // Player statuses above the bar.
    if let Some(bag) = eng.world.get::<StatusBag>(gs.pc.entity).cloned() {
        let mut x = bx;
        for s in &bag.active {
            let c = crate::vocab::status_color(s.id);
            ui.rect(Vec2::new(x, by - 22.0), Vec2::new(16.0, 16.0), c4(c));
            ui.text(Vec2::new(x + 2.0, by - 20.0), 1.4, Vec4::new(0.0, 0.0, 0.0, 1.0), &format!("{}", s.stacks));
            x += 20.0;
        }
    }
    if gs.pc.discorded > 0 {
        ui.text_shadowed(Vec2::new(bx, by - 44.0), 1.6, c4(palette::BLOOD), "DISCORDED: +DMG, PROCS MAY TURN");
    }

    // Skill bar (bottom center).
    let keys = ["LMB", "RMB", "Q", "E", "R", "F"];
    let slot_w = 74.0;
    let total = slot_w * 6.0;
    let sx = vp.x * 0.5 - total * 0.5;
    let sy = vp.y - 92.0;
    for i in 0..6 {
        let x = sx + i as f32 * slot_w;
        ui.rect(Vec2::new(x, sy), Vec2::new(slot_w - 6.0, 56.0), PANEL);
        match &gs.loadout.skills[i] {
            Some(id) => {
                let def = gs.db.skill(id).clone();
                let cd_max = def.cd_ticks.max(1) as f32;
                let cd = gs.pc.cooldowns[i] as f32;
                let ready = cd <= 0.0;
                let name_short: String = def.name.split(' ').last().unwrap_or("?").chars().take(9).collect();
                let color = if ready { c4(dominant_type(&def.dmg_vec()).color()) } else { DIM };
                ui.text(Vec2::new(x + 4.0, sy + 6.0), 1.3, color, &name_short);
                // Cooldown fill from bottom.
                if !ready {
                    let f = (cd / cd_max).clamp(0.0, 1.0);
                    ui.rect(Vec2::new(x, sy + 56.0 - 52.0 * f), Vec2::new(slot_w - 6.0, 52.0 * f), Vec4::new(0.0, 0.0, 0.0, 0.55));
                }
                // Sigil pips.
                let socketed = gs.loadout.sigils[i].iter().flatten().count();
                for s in 0..socketed {
                    ui.rect(Vec2::new(x + 4.0 + s as f32 * 10.0, sy + 44.0), Vec2::new(7.0, 7.0), c4(palette::GOLD));
                }
            }
            None => {
                ui.text(Vec2::new(x + 4.0, sy + 8.0), 1.3, DIM, "EMPTY");
            }
        }
        ui.text(Vec2::new(x + 4.0, sy + 24.0), 1.5, WHITE, keys[i]);
    }
    // Dodge pip.
    let dodge_ready = gs.pc.dodge_cd == 0 || gs.build.has_rule(&Rule::BloodDodge);
    ui.text(
        Vec2::new(sx + total + 8.0, sy + 8.0),
        1.5,
        if dodge_ready { c4(palette::BONE) } else { DIM },
        "SPACE",
    );

    // Right block: realm, tier, dust, kills.
    let rx = vp.x - 250.0;
    ui.text_shadowed(Vec2::new(rx, vp.y - 88.0), 1.6, c4(rs.demon.signature()), rs.demon.name());
    ui.text_shadowed(Vec2::new(rx, vp.y - 68.0), 1.5, WHITE, &format!("TIER {}", gs.tier));
    ui.text_shadowed(Vec2::new(rx, vp.y - 50.0), 1.5, c4(palette::GOLD), &format!("DUST {}", gs.dust));
    ui.text_shadowed(Vec2::new(rx, vp.y - 32.0), 1.5, DIM, &format!("KILLS {}", gs.pc.kills_this_run));

    // Realm modifiers (top right, small).
    let mut y = 46.0;
    for md in &rs.mods {
        ui.text_shadowed(Vec2::new(vp.x - 8.0 - UiBatch::text_width(1.2, &md.text), y), 1.2, DIM, &md.text);
        y += 15.0;
    }
    // Buer cycle banner.
    if rs.demon == Demon::Buer {
        let (t, c) = if gs.blight_phase { ("BLIGHT", palette::ICHOR) } else { ("BLOOM", palette::GOLD) };
        ui.text_shadowed(Vec2::new(vp.x - 8.0 - UiBatch::text_width(2.0, t), y + 6.0), 2.0, c4(c), t);
    }

    // Boss bar.
    if eng.world.is_alive(rs.boss) {
        if let Some(h) = eng.world.get::<Health>(rs.boss).copied() {
            let engaged = h.hp < h.max
                || eng
                    .world
                    .get::<Pos>(rs.boss)
                    .zip(eng.world.get::<Pos>(gs.pc.entity))
                    .map(|(b, p)| b.0.distance(p.0) < 28.0)
                    .unwrap_or(false);
            if engaged {
                let name = gs.db.enemy(&gs.db.realm(rs.demon).boss).name.clone();
                let w = 520.0;
                let x = vp.x * 0.5 - w * 0.5;
                ui.rect(Vec2::new(x - 2.0, 22.0), Vec2::new(w + 4.0, 22.0), PANEL);
                ui.rect(
                    Vec2::new(x, 24.0),
                    Vec2::new(w * (h.hp / h.max).clamp(0.0, 1.0), 18.0),
                    c4(rs.demon.signature()),
                );
                let immune = rs.demon == Demon::Buer && !gs.blight_phase;
                let label = if immune { format!("{name} - IMMUNE UNTIL BLIGHT") } else { name };
                ui.text_shadowed(
                    Vec2::new(vp.x * 0.5 - UiBatch::text_width(1.4, &label) * 0.5, 4.0),
                    1.4,
                    WHITE,
                    &label,
                );
            }
        }
    }

    // Contextual hints.
    let pp = eng.world.get::<Pos>(gs.pc.entity).map(|p| p.0).unwrap_or(Vec2::ZERO);
    let mut hint: Option<String> = None;
    if rs.portal_out.map(|p| pp.distance(p) < 3.0).unwrap_or(false) {
        hint = Some("G: BANK LOOT AND RETURN TO COURT".into());
    } else if pp.distance(rs.entry) < 2.5 {
        hint = Some("G: RETURN TO COURT (BANKS LOOT)".into());
    } else if pp.distance(rs.altar) < 3.0 {
        hint = Some("G: CORRUPT LAST PICKUP (10 DUST) - IT MAY NOT SURVIVE".into());
    } else {
        for (spos, used) in &rs.shrines {
            if !used && pp.distance(*spos) < 3.0 {
                hint = Some("G: UNVEIL HIDDEN AFFIXES".into());
            }
        }
    }
    if let Some(h) = hint {
        ui.text_shadowed(
            Vec2::new(vp.x * 0.5 - UiBatch::text_width(1.8, &h) * 0.5, vp.y - 140.0),
            1.8,
            c4(palette::GOLD),
            &h,
        );
    }
    if rs.cleared {
        let t = "THE SEAT IS EMPTY. TAKE WHAT IS OWED.";
        ui.text_shadowed(Vec2::new(vp.x * 0.5 - UiBatch::text_width(1.6, t) * 0.5, 52.0), 1.6, c4(palette::GOLD), t);
    }
    ui.text_shadowed(Vec2::new(8.0, vp.y - 20.0), 1.2, DIM, "TAB: SPOILS  WASD: MOVE  MOUSE: AIM  SPACE: DODGE  G: INTERACT");
}

pub fn draw_hud_court(eng: &mut Engine, gs: &mut Gs, ui: &mut UiBatch, sel: usize, tier: u32, progress: &crate::save::Progress) {
    let vp = eng.viewport;
    let title = "THE GOETIC COURT";
    ui.text_shadowed(Vec2::new(vp.x * 0.5 - UiBatch::text_width(3.0, title) * 0.5, 18.0), 3.0, WHITE, title);
    let sub = "72 SEATS. 69 DARK. THREE WAIT.";
    ui.text_shadowed(Vec2::new(vp.x * 0.5 - UiBatch::text_width(1.4, sub) * 0.5, 46.0), 1.4, DIM, sub);

    // Demon selector.
    let y = vp.y - 150.0;
    for d in DEMONS {
        let i = d.index();
        let x = vp.x * 0.5 + (i as f32 - 1.0) * 320.0 - 130.0;
        let selc = if i == sel { c4(d.signature()) } else { DIM };
        ui.rect(Vec2::new(x - 6.0, y - 6.0), Vec2::new(272.0, 64.0), PANEL);
        ui.text_shadowed(Vec2::new(x, y), 2.0, selc, &format!("{} {}", i + 1, d.name()));
        ui.text(Vec2::new(x, y + 22.0), 1.2, DIM, d.title());
        ui.text(
            Vec2::new(x, y + 40.0),
            1.3,
            if progress.cleared[i] > 0 { c4(palette::GOLD) } else { DIM },
            &format!("CLEARED: TIER {}", progress.cleared[i]),
        );
    }
    let line = format!(
        "TIER {tier}   [+/-] TIER   [1-3] DEMON   [ENTER] DESCEND   [TAB] SPOILS   DUST {}",
        gs.dust
    );
    ui.text_shadowed(
        Vec2::new(vp.x * 0.5 - UiBatch::text_width(1.7, &line) * 0.5, vp.y - 60.0),
        1.7,
        WHITE,
        &line,
    );
}

// ------------------------------------------------------------------- menu

#[derive(Clone, Copy, PartialEq)]
pub enum Pane {
    Inventory,
    Equipment,
    Skills,
    Contracts,
}

pub struct MenuState {
    pub open: bool,
    pub pane: Pane,
    pub cursor: usize,
    pub bench_family: usize,
    pub dirty: bool, // loadout changed → recompile + save
}

impl Default for MenuState {
    fn default() -> Self {
        MenuState { open: false, pane: Pane::Inventory, cursor: 0, bench_family: 0, dirty: false }
    }
}

const BENCH_FAMILIES: [(&str, &str); 6] = [
    ("DAMAGE", "dmg_g1"),
    ("LIFE", "hp1"),
    ("CRIT", "crit1"),
    ("CAST SPEED", "cast1"),
    ("STATUS", "status1"),
    ("LOOT", "quant1"),
];

fn inv_len(gs: &Gs, in_run: bool) -> usize {
    if in_run { gs.run_inv.len() } else { gs.bank.len() }
}

fn inv_item<'a>(gs: &'a Gs, in_run: bool, i: usize) -> Option<&'a ItemInstance> {
    if in_run { gs.run_inv.get(i) } else { gs.bank.get(i) }
}

fn inv_remove(gs: &mut Gs, in_run: bool, i: usize) -> Option<ItemInstance> {
    let v = if in_run { &mut gs.run_inv } else { &mut gs.bank };
    if i < v.len() { Some(v.remove(i)) } else { None }
}

fn inv_push(gs: &mut Gs, in_run: bool, item: ItemInstance) {
    if in_run { gs.run_inv.push(item) } else { gs.bank.push(item) }
}

/// Handle menu input for one tick. Returns true if the loadout changed.
pub fn menu_input(eng: &mut Engine, gs: &mut Gs, ms: &mut MenuState, in_run: bool) -> bool {
    if eng.input.key_pressed(KeyCode::Tab) {
        ms.open = !ms.open;
        eng.audio.play(&gs.sounds.ui, "sfx", 0.4, if ms.open { 1.2 } else { 0.8 });
    }
    if !ms.open {
        return false;
    }
    let panes = [Pane::Inventory, Pane::Equipment, Pane::Skills, Pane::Contracts];
    let pane_idx = panes.iter().position(|p| *p == ms.pane).unwrap_or(0);
    if eng.input.key_pressed(KeyCode::ArrowRight) {
        ms.pane = panes[(pane_idx + 1) % 4];
        ms.cursor = 0;
    }
    if eng.input.key_pressed(KeyCode::ArrowLeft) {
        ms.pane = panes[(pane_idx + 3) % 4];
        ms.cursor = 0;
    }
    let list_len = match ms.pane {
        Pane::Inventory => inv_len(gs, in_run).max(1),
        Pane::Equipment => EQUIP_SLOTS,
        Pane::Skills => 6,
        Pane::Contracts => 3,
    };
    if eng.input.key_pressed(KeyCode::ArrowDown) {
        ms.cursor = (ms.cursor + 1) % list_len;
    }
    if eng.input.key_pressed(KeyCode::ArrowUp) {
        ms.cursor = (ms.cursor + list_len - 1) % list_len;
    }
    ms.cursor = ms.cursor.min(list_len.saturating_sub(1));

    let mut changed = false;
    let enter = eng.input.key_pressed(KeyCode::Enter);
    match ms.pane {
        Pane::Inventory => {
            if enter {
                if let Some(item) = inv_remove(gs, in_run, ms.cursor) {
                    // Equip: rings prefer an empty hand.
                    let slot_idx = match item.slot {
                        Slot::Weapon => 0,
                        Slot::Armor => 1,
                        Slot::Relic => 2,
                        Slot::Ring => {
                            if gs.loadout.equipment[3].is_none() { 3 }
                            else if gs.loadout.equipment[4].is_none() { 4 }
                            else { 3 }
                        }
                    };
                    let old = gs.loadout.equipment[slot_idx].take();
                    gs.loadout.equipment[slot_idx] = Some(item);
                    if let Some(old) = old {
                        inv_push(gs, in_run, old);
                    }
                    changed = true;
                }
            }
            if eng.input.key_pressed(KeyCode::KeyX) {
                if let Some(item) = inv_remove(gs, in_run, ms.cursor) {
                    let dust = match item.rarity {
                        Rarity::Common => 1,
                        Rarity::Magic => 3,
                        Rarity::Rare => 8,
                        Rarity::Goetic => 25,
                    };
                    gs.dust += dust;
                    eng.audio.play(&gs.sounds.dust, "sfx", 0.4, 0.9);
                }
            }
            if !in_run {
                if eng.input.key_pressed(KeyCode::KeyN) {
                    ms.bench_family = (ms.bench_family + 1) % BENCH_FAMILIES.len();
                    eng.audio.play(&gs.sounds.ui, "sfx", 0.3, 1.0);
                }
                if eng.input.key_pressed(KeyCode::KeyV) && gs.dust >= 25 {
                    let (_, affix_id) = BENCH_FAMILIES[ms.bench_family];
                    let def = gs.db.affix(affix_id).clone();
                    let idx = ms.cursor;
                    let ok = {
                        let v = if in_run { &mut gs.run_inv } else { &mut gs.bank };
                        if let Some(item) = v.get_mut(idx) {
                            if def.slots.contains(&item.slot) && item.rarity != Rarity::Goetic {
                                item.affixes.push(AffixRoll {
                                    def_id: def.id.clone(),
                                    // Deterministic bench: exact mid-roll, every time.
                                    value: (def.lo + def.hi) * 0.5,
                                    hidden: false,
                                    revealed: true,
                                    corrupt: false,
                                });
                                if item.rarity == Rarity::Common {
                                    item.rarity = Rarity::Magic;
                                }
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };
                    if ok {
                        gs.dust -= 25;
                        eng.audio.play(&gs.sounds.corrupt, "sfx", 0.5, 1.4);
                    }
                }
            }
        }
        Pane::Equipment => {
            if enter {
                if let Some(item) = gs.loadout.equipment[ms.cursor].take() {
                    inv_push(gs, in_run, item);
                    changed = true;
                }
            }
        }
        Pane::Skills => {
            if in_run {
                // Locked mid-run; respec is a Court sacrament.
            } else {
                if enter {
                    // Cycle skill: None → each of the 8 → None.
                    let ids: Vec<String> = gs.db.skills().iter().map(|s| s.id.clone()).collect();
                    let cur = gs.loadout.skills[ms.cursor].clone();
                    let next = match cur {
                        None => ids.first().cloned(),
                        Some(c) => {
                            let i = ids.iter().position(|x| *x == c).unwrap_or(0);
                            if i + 1 < ids.len() { Some(ids[i + 1].clone()) } else { None }
                        }
                    };
                    gs.loadout.skills[ms.cursor] = next;
                    changed = true;
                }
                for (k, sock) in [(KeyCode::Digit1, 0), (KeyCode::Digit2, 1), (KeyCode::Digit3, 2)] {
                    if eng.input.key_pressed(k) {
                        let ids: Vec<String> = gs.db.sigils().iter().map(|s| s.id.clone()).collect();
                        let cur = gs.loadout.sigils[ms.cursor][sock].clone();
                        let next = match cur {
                            None => ids.first().cloned(),
                            Some(c) => {
                                let i = ids.iter().position(|x| *x == c).unwrap_or(0);
                                if i + 1 < ids.len() { Some(ids[i + 1].clone()) } else { None }
                            }
                        };
                        gs.loadout.sigils[ms.cursor][sock] = next;
                        changed = true;
                    }
                }
            }
        }
        Pane::Contracts => {
            if !in_run && enter {
                let ids: Vec<String> = gs.db.contracts().iter().map(|c| c.id.clone()).collect();
                let cur = gs.loadout.contracts[ms.cursor].clone();
                let next = match cur {
                    None => ids.first().cloned(),
                    Some(c) => {
                        let i = ids.iter().position(|x| *x == c).unwrap_or(0);
                        if i + 1 < ids.len() { Some(ids[i + 1].clone()) } else { None }
                    }
                };
                // No duplicate contracts across slots.
                let next = match next {
                    Some(n) if gs.loadout.contracts.iter().flatten().any(|c| *c == n) => {
                        let mut n2 = Some(n);
                        for _ in 0..ids.len() {
                            n2 = match n2 {
                                Some(c) => {
                                    let i = ids.iter().position(|x| *x == c).unwrap_or(0);
                                    if i + 1 < ids.len() { Some(ids[i + 1].clone()) } else { None }
                                }
                                None => break,
                            };
                            if let Some(ref c) = n2 {
                                if !gs.loadout.contracts.iter().flatten().any(|x| x == c) {
                                    break;
                                }
                            }
                        }
                        n2
                    }
                    other => other,
                };
                gs.loadout.contracts[ms.cursor] = next;
                changed = true;
            }
        }
    }
    if changed {
        gs.recompile();
        ms.dirty = true;
        eng.audio.play(&gs.sounds.ui, "sfx", 0.4, 1.1);
        // Live max-hp adjustment.
        let max = gs.build.sheet.get(K_MAX_HP);
        if let Some(h) = eng.world.get_mut::<Health>(gs.pc.entity) {
            let frac = (h.hp / h.max).clamp(0.0, 1.0);
            h.max = max;
            h.hp = max * frac;
        }
    }
    changed
}

pub fn draw_menu(eng: &mut Engine, gs: &mut Gs, ms: &MenuState, ui: &mut UiBatch, in_run: bool) {
    if !ms.open {
        return;
    }
    let vp = eng.viewport;
    let w = vp.x.min(1500.0) - 80.0;
    let h = vp.y - 140.0;
    let x0 = (vp.x - w) * 0.5;
    let y0 = 70.0;
    ui.rect(Vec2::new(x0, y0), Vec2::new(w, h), PANEL);

    // Pane tabs.
    let tabs = [("SPOILS", Pane::Inventory), ("VESTMENTS", Pane::Equipment), ("RITES", Pane::Skills), ("PACTS", Pane::Contracts)];
    let mut tx = x0 + 16.0;
    for (name, p) in tabs {
        let sel = ms.pane == p;
        ui.text(Vec2::new(tx, y0 + 12.0), 2.0, if sel { c4(palette::GOLD) } else { DIM }, name);
        tx += UiBatch::text_width(2.0, name) + 34.0;
    }
    ui.text(
        Vec2::new(x0 + w - 320.0, y0 + 12.0),
        1.4,
        DIM,
        "ARROWS: MOVE  ENTER: USE  TAB: CLOSE",
    );

    let list_x = x0 + 16.0;
    let list_y = y0 + 52.0;
    let line_h = 20.0;
    let card_x = x0 + w * 0.55;
    let all_hidden = gs.build.has_rule(&Rule::AllHiddenActive);

    match ms.pane {
        Pane::Inventory => {
            let n = inv_len(gs, in_run);
            ui.text(Vec2::new(list_x, list_y - 24.0), 1.3, DIM,
                if in_run { "RUN SPOILS (UNBANKED - DEATH EATS THESE)" } else { "BANKED SPOILS" });
            if n == 0 {
                ui.text(Vec2::new(list_x, list_y), 1.5, DIM, "NOTHING. GO TAKE SOMETHING.");
            }
            let first = ms.cursor.saturating_sub(14);
            for (row, i) in (first..n.min(first + 22)).enumerate() {
                let item = inv_item(gs, in_run, i).unwrap();
                let sel = i == ms.cursor;
                let c = if sel { c4(item.rarity.color()) } else { c4(item.rarity.color() * 0.6) };
                let marker = if sel { ">" } else { " " };
                ui.text(
                    Vec2::new(list_x, list_y + row as f32 * line_h),
                    1.5,
                    c,
                    &format!("{marker} {}", item.name),
                );
            }
            // Selected item card.
            if let Some(item) = inv_item(gs, in_run, ms.cursor) {
                let lines = item_lines(&gs.db, item, all_hidden);
                for (li, (text, col)) in lines.iter().enumerate() {
                    ui.text(Vec2::new(card_x, list_y + li as f32 * line_h), 1.5, c4(*col), text);
                }
            }
            let mut help = String::from("ENTER: EQUIP   X: SHATTER TO DUST");
            if !in_run {
                let (fam, _) = BENCH_FAMILIES[ms.bench_family];
                help.push_str(&format!("   V: BENCH-ADD {fam} (25 DUST)   N: NEXT FAMILY"));
            }
            ui.text(Vec2::new(list_x, y0 + h - 28.0), 1.4, c4(palette::GOLD), &help);
        }
        Pane::Equipment => {
            for i in 0..EQUIP_SLOTS {
                let sel = i == ms.cursor;
                let label = slot_name(equip_slot_kind(i));
                let (name, color) = match &gs.loadout.equipment[i] {
                    Some(it) => (it.name.clone(), c4(it.rarity.color())),
                    None => ("---".into(), DIM),
                };
                ui.text(
                    Vec2::new(list_x, list_y + i as f32 * line_h * 1.4),
                    1.6,
                    if sel { WHITE } else { DIM },
                    &format!("{} {label:<7}", if sel { ">" } else { " " }),
                );
                ui.text(Vec2::new(list_x + 130.0, list_y + i as f32 * line_h * 1.4), 1.6, color, &name);
            }
            if let Some(Some(item)) = gs.loadout.equipment.get(ms.cursor) {
                let lines = item_lines(&gs.db, item, all_hidden);
                for (li, (text, col)) in lines.iter().enumerate() {
                    ui.text(Vec2::new(card_x, list_y + li as f32 * line_h), 1.5, c4(*col), text);
                }
            }
            ui.text(Vec2::new(list_x, y0 + h - 28.0), 1.4, c4(palette::GOLD), "ENTER: UNEQUIP");
            // Stat readout.
            let stats = [
                ("LIFE", K_MAX_HP, false),
                ("DMG", K_DMG, true),
                ("CRIT", K_CRIT, true),
                ("CAST", K_CAST_SPEED, true),
                ("MOVE", K_SPEED, true),
                ("STATUS", K_STATUS_CHANCE, true),
                ("RARITY", K_LOOT_RARE, true),
            ];
            let sy = list_y + 200.0;
            ui.text(Vec2::new(list_x, sy - 24.0), 1.3, DIM, "THE SUM OF YOU:");
            for (i, (name, key, pct)) in stats.iter().enumerate() {
                let v = gs.build.sheet.get(*key);
                let txt = if *pct {
                    format!("{name} {:+.0}%", v * 100.0)
                } else {
                    format!("{name} {v:.0}")
                };
                ui.text(Vec2::new(list_x + (i % 4) as f32 * 160.0, sy + (i / 4) as f32 * 22.0), 1.4, WHITE, &txt);
            }
        }
        Pane::Skills => {
            if in_run {
                ui.text(Vec2::new(list_x, list_y), 1.6, DIM, "RITES ARE REWRITTEN AT COURT ONLY.");
            }
            let keys = ["LMB", "RMB", "Q", "E", "R", "F"];
            for i in 0..6 {
                let sel = i == ms.cursor;
                let y = list_y + i as f32 * line_h * 2.6;
                let (name, desc) = match &gs.loadout.skills[i] {
                    Some(id) => {
                        let d = gs.db.skill(id);
                        (d.name.clone(), d.desc.clone())
                    }
                    None => ("---".into(), String::new()),
                };
                ui.text(Vec2::new(list_x, y), 1.6, if sel { WHITE } else { DIM }, &format!("{} {:<4} {}", if sel { ">" } else { " " }, keys[i], name));
                ui.text(Vec2::new(list_x + 60.0, y + 18.0), 1.2, DIM, &desc);
                // Sigil sockets.
                let mut sx2 = list_x + 620.0;
                for s in 0..3 {
                    let txt = match &gs.loadout.sigils[i][s] {
                        Some(sid) => gs.db.sigil(sid).name.replace("SIGIL OF ", ""),
                        None => "( )".into(),
                    };
                    let c = if gs.loadout.sigils[i][s].is_some() { c4(palette::GOLD) } else { DIM };
                    ui.text(Vec2::new(sx2, y), 1.3, c, &format!("[{}] {txt}", s + 1));
                    sx2 += 210.0;
                }
            }
            if !in_run {
                ui.text(Vec2::new(list_x, y0 + h - 28.0), 1.4, c4(palette::GOLD), "ENTER: CYCLE RITE   1/2/3: CYCLE SIGIL SOCKETS   RESPEC IS FREE. ALWAYS.");
            }
        }
        Pane::Contracts => {
            if in_run {
                ui.text(Vec2::new(list_x, list_y), 1.6, DIM, "PACTS ARE SIGNED AT COURT ONLY.");
            }
            for i in 0..3 {
                let sel = i == ms.cursor;
                let y = list_y + i as f32 * line_h * 3.4;
                match &gs.loadout.contracts[i] {
                    Some(id) => {
                        let c = gs.db.contract(id).clone();
                        ui.text(Vec2::new(list_x, y), 1.6, if sel { c4(c.demon.signature()) } else { DIM }, &format!("{} {}", if sel { ">" } else { " " }, c.name));
                        ui.text(Vec2::new(list_x + 30.0, y + 18.0), 1.2, WHITE, &c.text);
                        ui.text(Vec2::new(list_x + 30.0, y + 34.0), 1.1, DIM, &format!("- {}", c.demon.name()));
                    }
                    None => {
                        ui.text(Vec2::new(list_x, y), 1.6, if sel { WHITE } else { DIM }, &format!("{} UNSIGNED", if sel { ">" } else { " " }));
                    }
                }
            }
            if !in_run {
                ui.text(Vec2::new(list_x, y0 + h - 28.0), 1.4, c4(palette::GOLD), "ENTER: CYCLE PACT. THREE AT MOST. READ THE FINE PRINT OR DON'T.");
            }
        }
    }
}

pub fn draw_death(eng: &mut Engine, ui: &mut UiBatch, timer: u16, lost: usize) {
    let vp = eng.viewport;
    ui.rect(Vec2::ZERO, vp, Vec4::new(0.04, 0.0, 0.01, 0.6));
    let t = "THE COURT RECLAIMS YOU";
    ui.text_shadowed(Vec2::new(vp.x * 0.5 - UiBatch::text_width(3.4, t) * 0.5, vp.y * 0.4), 3.4, c4(palette::BLOOD), t);
    let l = format!("{lost} UNBANKED SPOILS FORFEIT. THE BANK REMEMBERS.");
    ui.text_shadowed(Vec2::new(vp.x * 0.5 - UiBatch::text_width(1.6, &l) * 0.5, vp.y * 0.4 + 44.0), 1.6, WHITE, &l);
    let s = format!("RETURNING IN {}", (timer / 60) + 1);
    ui.text_shadowed(Vec2::new(vp.x * 0.5 - UiBatch::text_width(1.6, &s) * 0.5, vp.y * 0.4 + 70.0), 1.6, DIM, &s);
}
