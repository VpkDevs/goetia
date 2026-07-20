//! World rendering: meshes, instances, lights, per-realm dressing. The
//! visual hierarchy (Pillar 3) is enforced through emissive intensity:
//! player > telegraphs > loot beams > player VFX > enemy VFX > environment.

use crate::combat::*;
use crate::content::EnemyShape;
use crate::run::{RunState, CELL};
use crate::vocab::*;
use goetia::prelude::*;

pub struct Meshes {
    pub ground: MeshHandle,
    pub slab: MeshHandle,
    pub fiend: MeshHandle, // spike silhouette
    pub golem: MeshHandle, // heavy stacked mass
    pub orb: MeshHandle,
    pub wheel: MeshHandle,
    pub column: MeshHandle,
    pub pillar: MeshHandle, // environment
    pub player: MeshHandle,
    pub portal: MeshHandle,
    pub shrine: MeshHandle,
    pub altar: MeshHandle,
    pub seat: MeshHandle, // court thrones
    pub beam: MeshHandle, // loot beam
    pub disc: MeshHandle, // zone disc
}

pub fn register_meshes(gfx: &mut Renderer) -> Meshes {
    Meshes {
        ground: gfx.register_mesh(MeshBuilder::ground(1.0, 1.0)),
        slab: gfx.register_mesh(MeshBuilder::cube()),
        fiend: gfx.register_mesh(
            MeshBuilder::spike(1.6)
                .merged(
                    MeshBuilder::spike(0.8)
                        .rotated(Quat::from_rotation_z(2.5))
                        .translated(Vec3::new(0.4, 0.9, 0.0)),
                )
                .merged(
                    MeshBuilder::spike(0.8)
                        .rotated(Quat::from_rotation_z(-2.5))
                        .translated(Vec3::new(-0.4, 0.9, 0.0)),
                )
                .jittered(0.05),
        ),
        golem: gfx.register_mesh(
            MeshBuilder::boxed(Vec3::new(-0.5, 0.0, -0.4), Vec3::new(0.5, 1.1, 0.4))
                .merged(MeshBuilder::boxed(
                    Vec3::new(-0.35, 1.1, -0.3),
                    Vec3::new(0.35, 1.6, 0.3),
                ))
                .merged(
                    MeshBuilder::cube()
                        .scaled(Vec3::splat(0.5))
                        .translated(Vec3::new(0.0, 1.85, 0.0)),
                )
                .jittered(0.04),
        ),
        orb: gfx.register_mesh(MeshBuilder::orb(1, 0.5)),
        wheel: gfx.register_mesh(
            MeshBuilder::prism(10, 0.8, 0.3)
                .rotated(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
                .translated(Vec3::new(0.0, 0.8, 0.0)),
        ),
        column: gfx.register_mesh(MeshBuilder::column(1.8).tapered(0.7).jittered(0.03)),
        pillar: gfx.register_mesh(
            MeshBuilder::column(3.4)
                .twisted(0.08)
                .tapered(0.55)
                .jittered(0.03),
        ),
        player: gfx.register_mesh(
            MeshBuilder::spike(1.7)
                .scaled(Vec3::new(0.7, 1.0, 0.7))
                .merged(MeshBuilder::orb(1, 0.22).translated(Vec3::new(0.0, 1.9, 0.0))),
        ),
        portal: gfx.register_mesh(
            MeshBuilder::prism(8, 1.2, 0.2)
                .merged(MeshBuilder::orb(2, 0.6).translated(Vec3::new(0.0, 1.4, 0.0))),
        ),
        shrine: gfx.register_mesh(
            MeshBuilder::column(1.4)
                .tapered(1.3)
                .merged(MeshBuilder::orb(1, 0.35).translated(Vec3::new(0.0, 1.8, 0.0))),
        ),
        altar: gfx.register_mesh(
            MeshBuilder::boxed(Vec3::new(-0.9, 0.0, -0.5), Vec3::new(0.9, 0.7, 0.5))
                .merged(
                    MeshBuilder::spike(1.0)
                        .scaled(Vec3::new(0.3, 1.0, 0.3))
                        .translated(Vec3::new(0.0, 0.7, 0.0)),
                )
                .jittered(0.03),
        ),
        seat: gfx.register_mesh(
            MeshBuilder::column(2.6)
                .tapered(0.8)
                .merged(MeshBuilder::boxed(
                    Vec3::new(-0.5, 0.0, -0.4),
                    Vec3::new(0.5, 1.0, 0.4),
                )),
        ),
        beam: gfx.register_mesh(MeshBuilder::column(6.0).scaled(Vec3::new(0.16, 1.0, 0.16))),
        disc: gfx.register_mesh(MeshBuilder::prism(20, 1.0, 0.06)),
    }
}

fn shape_mesh(m: &Meshes, s: EnemyShape) -> MeshHandle {
    match s {
        EnemyShape::Spike => m.fiend,
        EnemyShape::Golem => m.golem,
        EnemyShape::Orb => m.orb,
        EnemyShape::Wheel => m.wheel,
        EnemyShape::Column => m.column,
    }
}

struct Batches {
    per_mesh: Vec<(MeshHandle, Vec<InstanceRaw>)>,
}

impl Batches {
    fn new() -> Self {
        Batches {
            per_mesh: Vec::new(),
        }
    }
    fn push(&mut self, m: MeshHandle, i: InstanceRaw) {
        if let Some((_, v)) = self.per_mesh.iter_mut().find(|(h, _)| *h == m) {
            v.push(i);
        } else {
            self.per_mesh.push((m, vec![i]));
        }
    }
    fn submit(self, frame: &mut FrameSubmit) {
        for (h, v) in self.per_mesh {
            frame.meshes.push((h, v));
        }
    }
}

fn lerp_pos(eng: &mut Engine, e: Entity, alpha: f32) -> Option<Vec2> {
    let p = eng.world.get::<Pos>(e)?.0;
    let pp = eng.world.get::<PrevPos>(e).map(|x| x.0).unwrap_or(p);
    Some(pp.lerp(p, alpha))
}

// ------------------------------------------------------------------- run

pub fn draw_run(
    eng: &mut Engine,
    gs: &mut Gs,
    rs: &RunState,
    m: &Meshes,
    frame: &mut FrameSubmit,
    alpha: f32,
) {
    let sig = rs.demon.signature();
    let realm = gs.db.realm(rs.demon).clone();
    let mut b = Batches::new();

    // Realm mood.
    frame.ambient = Vec3::new(0.10, 0.09, 0.13) * realm.ambient;
    if rs.demon == Demon::Buer {
        let t = if gs.blight_phase {
            palette::ICHOR
        } else {
            palette::GOLD
        };
        frame.fog = (t * 0.03).extend(0.022);
    } else {
        frame.fog = (sig * 0.04 + palette::VOID).extend(0.02);
    }

    // Floors + corner pillars per room (environment: dim, non-emissive).
    let templates = gs.db.room_templates(rs.demon).to_vec();
    for (ri, room) in rs.layout.rooms.iter().enumerate() {
        let t = &templates[room.template];
        let w = t.width as f32 * CELL;
        let d = t.height as f32 * CELL;
        let cx = room.x as f32 * CELL + w * 0.5;
        let cz = room.y as f32 * CELL + d * 0.5;
        let shade = 0.55 + ((ri * 13) % 4) as f32 * 0.08;
        b.push(
            m.ground,
            InstanceRaw::new(
                Mat4::from_translation(Vec3::new(cx, 0.0, cz))
                    * Mat4::from_scale(Vec3::new(w, 1.0, d)),
                (palette::ASH * 0.5 * shade + sig * 0.02).extend(1.0),
            ),
        );
        for (px, pz) in [
            (room.x as f32 * CELL + 0.7, room.y as f32 * CELL + 0.7),
            (room.x as f32 * CELL + w - 0.7, room.y as f32 * CELL + 0.7),
            (room.x as f32 * CELL + 0.7, room.y as f32 * CELL + d - 0.7),
            (
                room.x as f32 * CELL + w - 0.7,
                room.y as f32 * CELL + d - 0.7,
            ),
        ] {
            b.push(
                m.pillar,
                InstanceRaw::new(
                    Mat4::from_translation(Vec3::new(px, 0.0, pz)),
                    palette::VOID.extend(1.0),
                )
                .emissive(sig * 0.12, 1.0)
                .phase(px + pz),
            );
        }
        // Connection floor patches.
    }
    for (_, _, cell) in &rs.layout.connections {
        b.push(
            m.ground,
            InstanceRaw::new(
                Mat4::from_translation(Vec3::new(
                    cell.0 as f32 * CELL + CELL * 0.5,
                    0.0,
                    cell.1 as f32 * CELL + CELL * 0.5,
                )) * Mat4::from_scale(Vec3::new(CELL, 1.0, CELL)),
                (palette::ASH * 0.35).extend(1.0),
            ),
        );
    }

    // Entry + exit portals.
    let portal_at =
        |at: Vec2, color: Vec3, strong: f32, b: &mut Batches, frame: &mut FrameSubmit| {
            b.push(
                m.portal,
                InstanceRaw::new(
                    Mat4::from_translation(Vec3::new(at.x, 0.0, at.y)),
                    palette::VOID.extend(1.0),
                )
                .emissive(color, strong)
                .wobble(0.06, 2.0)
                .phase(at.x),
            );
            frame.lights.push(Light {
                pos: Vec3::new(at.x, 1.5, at.y),
                color,
                radius: 7.0,
                intensity: strong,
            });
            frame.particle_spawns.push(ParticleSpawn {
                pos: Vec3::new(at.x, 0.3, at.y),
                count: 2,
                vel: Vec3::Y * 1.8,
                spread: 0.7,
                color_from: color.extend(0.8),
                color_to: color.extend(0.0),
                size: (0.04, 0.1),
                life: (0.6, 1.6),
                gravity: -0.3,
                drag: 0.5,
            });
        };
    portal_at(rs.entry, palette::BONE * 0.8, 1.2, &mut b, frame);
    if let Some(p) = rs.portal_out {
        portal_at(p, palette::GOLD, 2.2, &mut b, frame);
    }

    // Shrines + altar.
    for (spos, used) in &rs.shrines {
        let e = if *used { sig * 0.1 } else { palette::HEX * 1.6 };
        b.push(
            m.shrine,
            InstanceRaw::new(
                Mat4::from_translation(Vec3::new(spos.x, 0.0, spos.y)),
                palette::VOID.extend(1.0),
            )
            .emissive(e, 1.0)
            .wobble(0.03, 1.5),
        );
        if !*used && frame.lights.len() < 40 {
            frame.lights.push(Light {
                pos: Vec3::new(spos.x, 2.0, spos.y),
                color: palette::HEX,
                radius: 6.0,
                intensity: 1.3,
            });
        }
    }
    b.push(
        m.altar,
        InstanceRaw::new(
            Mat4::from_translation(Vec3::new(rs.altar.x, 0.0, rs.altar.y)),
            (palette::VOID * 2.0).extend(1.0),
        )
        .emissive(palette::BLOOD * 0.9, 1.0)
        .wobble(0.02, 3.0),
    );
    frame.lights.push(Light {
        pos: Vec3::new(rs.altar.x, 1.5, rs.altar.y),
        color: palette::BLOOD,
        radius: 5.0,
        intensity: 1.1,
    });

    draw_actors(eng, gs, m, &mut b, frame, alpha, sig);
    b.submit(frame);
    crate::fx::drain_fx(eng, frame);
}

/// Enemies, player, minions, totems, projectiles, zones, loot, corpses —
/// shared between run and court (court just has fewer of them).
fn draw_actors(
    eng: &mut Engine,
    gs: &mut Gs,
    m: &Meshes,
    b: &mut Batches,
    frame: &mut FrameSubmit,
    alpha: f32,
    sig: Vec3,
) {
    let time = eng.clock.tick as f32 / 60.0;

    // ---- enemies
    struct Draw {
        e: Entity,
        shape: EnemyShape,
        scale: f32,
        elite: bool,
        state: AiState,
        boss: bool,
        orbitspin: f32,
        phase: f32,
    }
    let mut list = Vec::new();
    eng.world.each::<(&EnemyC,)>(|ent, (ec,)| {
        let def = gs.db.enemy(&ec.def_id);
        list.push(Draw {
            e: ent,
            shape: def.shape,
            scale: def.scale * if ec.elite { 1.5 } else { 1.0 },
            elite: ec.elite,
            state: ec.state,
            boss: false,
            orbitspin: ec.orbit,
            phase: ec.phase,
        });
    });
    for d in &mut list {
        d.boss = eng.world.has::<BossC>(d.e);
    }
    for d in list {
        let Some(ip) = lerp_pos(eng, d.e, alpha) else {
            continue;
        };
        let bag = eng.world.get::<StatusBag>(d.e).cloned().unwrap_or_default();
        let hp_flash = eng.world.get::<Health>(d.e).map(|h| h.flash).unwrap_or(0.0);
        // Health flash decays render-side (sim only sets it).
        if let Some(h) = eng.world.get_mut::<Health>(d.e) {
            h.flash = (h.flash - 0.12).max(0.0);
        }

        let petrified = bag.has(ST_PETRIFY);
        let mut color = if petrified {
            palette::ASH * 2.2
        } else {
            palette::BLOOD * 0.75
        };
        let mut emissive = Vec3::ZERO;
        // Telegraph = the loudest thing an enemy is allowed to be (Pillar 3).
        if d.state == AiState::Telegraph {
            let pulse = (time * 18.0).sin() * 0.5 + 0.5;
            color = palette::BONE;
            emissive += palette::BONE * (1.2 + pulse * 1.6);
        }
        if bag.has(ST_IGNITE) {
            emissive += palette::BRIMSTONE * 0.7;
        }
        if bag.has(ST_BLIGHT) {
            emissive += palette::ICHOR * 0.5;
        }
        if bag.has(ST_DISCORD) {
            emissive += palette::BLOOD * ((time * 9.0).sin() * 0.3 + 0.5);
        }
        if bag.has(ST_HEXMARK) {
            emissive += palette::HEX * 0.6;
        }
        if d.elite {
            emissive += sig * 0.5;
            color += sig * 0.15;
        }
        if d.boss {
            emissive += sig * 0.9;
            frame.lights.push(Light {
                pos: Vec3::new(ip.x, 2.5, ip.y),
                color: sig,
                radius: 10.0,
                intensity: 1.8,
            });
        }
        emissive += Vec3::splat(hp_flash * 3.0); // hit flash: white pop
        let spin = if d.shape == EnemyShape::Wheel {
            Mat4::from_rotation_y(d.orbitspin * 3.0) * Mat4::from_rotation_x(time * 6.0)
        } else {
            Mat4::IDENTITY
        };
        b.push(
            shape_mesh(m, d.shape),
            InstanceRaw::new(
                Mat4::from_translation(Vec3::new(ip.x, 0.0, ip.y))
                    * Mat4::from_scale(Vec3::splat(d.scale))
                    * spin,
                color.extend(1.0),
            )
            .emissive(emissive, 1.0)
            .phase(d.phase)
            .wobble(if petrified { 0.0 } else { 0.05 }, 5.0 + d.phase),
        );
    }

    // ---- player (brightest persistent thing on screen)
    if let Some(ip) = lerp_pos(eng, gs.pc.entity, alpha) {
        let iframes = gs.pc.iframes > 0;
        let discorded = gs.pc.discorded > 0;
        let mut em = palette::HEX * 1.1 + Vec3::splat(0.15);
        if discorded {
            em = palette::BLOOD * 1.6;
        }
        if iframes {
            em *= 2.2;
        }
        let flash = eng
            .world
            .get::<Health>(gs.pc.entity)
            .map(|h| h.flash)
            .unwrap_or(0.0);
        if let Some(h) = eng.world.get_mut::<Health>(gs.pc.entity) {
            h.flash = (h.flash - 0.1).max(0.0);
        }
        b.push(
            m.player,
            InstanceRaw::new(
                Mat4::from_translation(Vec3::new(ip.x, 0.0, ip.y)),
                (palette::BONE + Vec3::splat(flash)).extend(1.0),
            )
            .emissive(em, 1.0)
            .wobble(0.04, 7.0),
        );
        frame.lights.push(Light {
            pos: Vec3::new(ip.x, 2.0, ip.y),
            color: if discorded {
                palette::BLOOD
            } else {
                palette::HEX
            },
            radius: 9.0,
            intensity: 1.6,
        });
    }

    // ---- minions & totems
    let mut servants: Vec<(Vec2, bool, f32)> = Vec::new();
    eng.world
        .each::<(&Pos, &MinionC)>(|_, (p, mc)| servants.push((p.0, false, mc.life as f32)));
    eng.world
        .each::<(&Pos, &TotemC)>(|_, (p, tc)| servants.push((p.0, true, tc.life as f32)));
    for (p, is_totem, life) in servants {
        let fade = (life / 60.0).min(1.0);
        b.push(
            if is_totem { m.column } else { m.fiend },
            InstanceRaw::new(
                Mat4::from_translation(Vec3::new(p.x, 0.0, p.y))
                    * Mat4::from_scale(Vec3::splat(if is_totem { 0.9 } else { 0.7 })),
                (palette::VOID * 2.0).extend(1.0),
            )
            .emissive(palette::HEX * 0.8 * fade, 1.0)
            .wobble(0.06, 8.0)
            .phase(p.x * 3.1),
        );
    }

    // ---- projectiles
    let mut proj_lights = 0;
    let mut projs: Vec<(Entity, bool, DmgVec, f32)> = Vec::new();
    eng.world
        .each::<(&Proj,)>(|e, (pr,)| projs.push((e, pr.friendly, pr.dmg, pr.radius)));
    for (e, friendly, dmg, radius) in projs {
        let Some(ip) = lerp_pos(eng, e, alpha) else {
            continue;
        };
        let c = dominant_type(&dmg).color();
        let c = if friendly {
            c
        } else {
            c * 0.9 + palette::BLOOD * 0.4
        };
        let pos = Vec3::new(ip.x, 0.8, ip.y);
        b.push(
            m.orb,
            InstanceRaw::new(
                Mat4::from_translation(pos) * Mat4::from_scale(Vec3::splat(radius * 2.6)),
                c.extend(1.0),
            )
            .emissive(c, if friendly { 1.8 } else { 1.4 }),
        );
        if proj_lights < 24 && frame.lights.len() < 58 {
            proj_lights += 1;
            frame.lights.push(Light {
                pos,
                color: c,
                radius: 4.0,
                intensity: 1.0,
            });
        }
        if eng.clock.tick.is_multiple_of(3) {
            frame.particle_spawns.push(ParticleSpawn {
                pos,
                count: 1,
                vel: Vec3::ZERO,
                spread: 0.2,
                color_from: c.extend(0.7),
                color_to: c.extend(0.0),
                size: (0.04, 0.09),
                life: (0.15, 0.35),
                gravity: 0.0,
                drag: 2.0,
            });
        }
    }

    // ---- zones (telegraphs pulse blood-red: unmissable)
    let mut zones: Vec<(Vec2, f32, bool, bool, bool, f32)> = Vec::new();
    eng.world.each::<(&Pos, &Zone)>(|_, (p, z)| {
        zones.push((
            p.0,
            z.radius,
            z.friendly,
            z.consecrate,
            z.telegraph_burst.is_some(),
            z.life as f32,
        ));
    });
    for (p, radius, friendly, consecrate, telegraph, life) in zones {
        let (c, e) = if telegraph {
            let pulse = (time * 20.0).sin() * 0.5 + 0.5;
            (
                palette::BLOOD,
                palette::BLOOD * (1.5 + pulse * 2.0) * (1.0 - life / 60.0).max(0.3),
            )
        } else if consecrate {
            (palette::GOLD * 0.6, palette::GOLD * 0.7)
        } else if friendly {
            (palette::HEX * 0.5, palette::HEX * 0.6)
        } else {
            (palette::BLOOD * 0.5, palette::BLOOD * 0.6)
        };
        b.push(
            m.disc,
            InstanceRaw::new(
                Mat4::from_translation(Vec3::new(p.x, 0.03, p.y))
                    * Mat4::from_scale(Vec3::new(radius, 1.0, radius)),
                c.extend(0.55),
            )
            .emissive(e, 1.0)
            .phase(p.x),
        );
    }

    // ---- loot beams: light IS information
    let mut drops: Vec<(Vec2, Vec3, bool)> = Vec::new();
    eng.world.each::<(&Pos, &LootDrop)>(|_, (p, l)| {
        let (c, big) = match &l.item {
            None => (palette::ASH * 3.0, false),
            Some(i) => (i.rarity.color(), i.rarity >= Rarity::Rare),
        };
        drops.push((p.0, c, big));
    });
    for (p, c, big) in drops {
        b.push(
            m.beam,
            InstanceRaw::new(
                Mat4::from_translation(Vec3::new(p.x, 0.0, p.y)),
                c.extend(if big { 0.8 } else { 0.45 }),
            )
            .emissive(c * if big { 2.0 } else { 0.8 }, 1.0)
            .wobble(0.02, 2.0)
            .phase(p.x * 7.0),
        );
        if big && frame.lights.len() < 62 {
            frame.lights.push(Light {
                pos: Vec3::new(p.x, 1.5, p.y),
                color: c,
                radius: 4.5,
                intensity: 1.6,
            });
        }
    }

    // ---- corpses
    let mut corpses: Vec<(Vec2, f32, f32, Vec3)> = Vec::new();
    eng.world.each::<(&Pos, &CorpseC)>(|_, (p, c)| {
        corpses.push((p.0, c.age as f32 / c.max_age as f32, c.scale, c.tint));
    });
    for (p, t, scale, tint) in corpses {
        b.push(
            m.slab,
            InstanceRaw::new(
                Mat4::from_translation(Vec3::new(p.x, -t * 0.3, p.y))
                    * Mat4::from_scale(Vec3::new(scale, 0.3 * scale, scale)),
                (tint * 0.5).extend(1.0),
            )
            .emissive(tint * (1.0 - t) * 0.2, 1.0)
            .dissolve(t * t),
        );
    }
}

// ------------------------------------------------------------------ court

pub fn draw_court(
    eng: &mut Engine,
    gs: &mut Gs,
    m: &Meshes,
    frame: &mut FrameSubmit,
    alpha: f32,
    sel_demon: usize,
) {
    let mut b = Batches::new();
    frame.ambient = Vec3::new(0.08, 0.07, 0.11);
    frame.fog = (palette::VOID * 1.5).extend(0.016);

    // Floor dais.
    b.push(
        m.ground,
        InstanceRaw::new(
            Mat4::from_scale(Vec3::new(70.0, 1.0, 70.0)),
            (palette::ASH * 0.35).extend(1.0),
        ),
    );
    b.push(
        m.disc,
        InstanceRaw::new(
            Mat4::from_scale(Vec3::new(8.0, 1.0, 8.0)),
            (palette::VOID * 3.0).extend(1.0),
        )
        .emissive(palette::HEX * 0.25, 1.0),
    );

    // 72 seats; three lit. The roadmap is furniture.
    let time = eng.clock.tick as f32 / 60.0;
    for i in 0..72 {
        let a = i as f32 / 72.0 * std::f32::consts::TAU;
        let r = 26.0;
        let pos = Vec3::new(a.cos() * r, 0.0, a.sin() * r);
        let lit = match i {
            0 => Some(Demon::Vassago),
            24 => Some(Demon::Andras),
            48 => Some(Demon::Buer),
            _ => None,
        };
        let (color, emissive) = match lit {
            Some(d) => {
                let selected = d.index() == sel_demon;
                let pulse = if selected {
                    (time * 4.0).sin() * 0.4 + 1.3
                } else {
                    0.7
                };
                (palette::VOID * 2.0, d.signature() * pulse)
            }
            None => (palette::VOID * 1.4, Vec3::ZERO),
        };
        b.push(
            m.seat,
            InstanceRaw::new(
                Mat4::from_translation(pos)
                    * Mat4::from_rotation_y(-a + std::f32::consts::FRAC_PI_2),
                color.extend(1.0),
            )
            .emissive(emissive, 1.0)
            .phase(i as f32),
        );
        if let Some(d) = lit {
            frame.lights.push(Light {
                pos: pos + Vec3::Y * 2.5,
                color: d.signature(),
                radius: 9.0,
                intensity: if d.index() == sel_demon { 2.2 } else { 1.0 },
            });
        }
    }

    // Three realm gates in front of the lit seats.
    for d in DEMONS {
        let a = d.index() as f32 / 3.0 * std::f32::consts::TAU;
        let at = Vec2::new(a.cos() * 14.0, a.sin() * 14.0);
        let selected = d.index() == sel_demon;
        b.push(
            m.portal,
            InstanceRaw::new(
                Mat4::from_translation(Vec3::new(at.x, 0.0, at.y)),
                palette::VOID.extend(1.0),
            )
            .emissive(d.signature() * if selected { 2.4 } else { 0.8 }, 1.0)
            .wobble(0.05, 2.0)
            .phase(d.index() as f32 * 2.0),
        );
        if selected {
            frame.particle_spawns.push(ParticleSpawn {
                pos: Vec3::new(at.x, 0.5, at.y),
                count: 3,
                vel: Vec3::Y * 2.0,
                spread: 0.8,
                color_from: d.signature().extend(0.9),
                color_to: d.signature().extend(0.0),
                size: (0.05, 0.11),
                life: (0.5, 1.4),
                gravity: -0.4,
                drag: 0.5,
            });
        }
    }

    draw_actors(eng, gs, m, &mut b, frame, alpha, palette::HEX);
    b.submit(frame);
    crate::fx::drain_fx(eng, frame);
}
