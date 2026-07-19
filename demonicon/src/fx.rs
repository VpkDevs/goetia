//! Juice bank: synthesized sounds + a queue of visual bursts the renderer
//! drains each frame. The feel budget lives here and is never raided.

use goetia::prelude::*;

pub struct Sounds {
    pub hit: Sound,
    pub crit: Sound,
    pub kill: Sound,
    pub hurt: Sound,
    pub cast: Sound,
    pub nova: Sound,
    pub ritual: Sound,
    pub summon: Sound,
    pub curse: Sound,
    pub beam: Sound,
    pub dodge: Sound,
    pub loot: Sound,
    pub goetic: Sound,
    pub dust: Sound,
    pub portal: Sound,
    pub death: Sound,
    pub boss_dead: Sound,
    pub ui: Sound,
    pub corrupt: Sound,
}

impl Sounds {
    pub fn synth() -> Sounds {
        Sounds {
            hit: Sound::noise_burst(0.07, 0.6),
            crit: Sound::blip(880.0, 0.08),
            kill: Sound::noise_burst(0.16, 0.45),
            hurt: Sound::noise_burst(0.2, 0.25),
            cast: Sound::blip(520.0, 0.06),
            nova: Sound::noise_burst(0.3, 0.35),
            ritual: Sound::blip(196.0, 0.25),
            summon: Sound::blip(147.0, 0.3),
            curse: Sound::blip(311.0, 0.2),
            beam: Sound::blip(660.0, 0.05),
            dodge: Sound::noise_burst(0.1, 0.8),
            loot: Sound::blip(784.0, 0.12),
            goetic: Sound::synth(0.7, 44100, |t| {
                // Rising infernal arpeggio.
                let f = [220.0, 277.2, 329.6, 440.0][((t * 6.0) as usize).min(3)];
                (t * f * std::f32::consts::TAU).sin() * (1.0 - t / 0.7).max(0.0) * 0.6
            }),
            dust: Sound::blip(1200.0, 0.03),
            portal: Sound::synth(0.6, 44100, |t| {
                ((t * 90.0 + t * t * 300.0) * std::f32::consts::TAU).sin()
                    * (1.0 - t / 0.6).max(0.0)
                    * 0.5
            }),
            death: Sound::synth(1.0, 44100, |t| {
                ((t * 150.0 - t * t * 80.0) * std::f32::consts::TAU).sin()
                    * (1.0 - t).max(0.0)
                    * 0.7
            }),
            boss_dead: Sound::noise_burst(0.9, 0.15),
            ui: Sound::blip(440.0, 0.04),
            corrupt: Sound::synth(0.5, 44100, |t| {
                let f = 300.0 - t * 200.0;
                (t * f * std::f32::consts::TAU).sin() * (1.0 - t / 0.5).max(0.0) * 0.6
            }),
        }
    }
}

/// Visual FX queued by sim code, drained by render_extract into particles.
#[derive(Default)]
pub struct FxQueue {
    /// (pos, color, count, spread)
    pub bursts: Vec<(Vec2, Vec3, u32, f32)>,
    /// (pos, radius, color) — expanding ring pulse.
    pub rings: Vec<(Vec2, f32, Vec3)>,
}

pub fn drain_fx(eng: &mut Engine, frame: &mut FrameSubmit) {
    let fx = std::mem::take(eng.world.resource_mut::<FxQueue>());
    for (at, color, count, spread) in fx.bursts {
        frame.particle_spawns.push(ParticleSpawn {
            pos: Vec3::new(at.x, 0.7, at.y),
            count,
            vel: Vec3::Y * 2.0,
            spread,
            color_from: color.extend(1.0),
            color_to: (color * 0.4).extend(0.4),
            size: (0.05, 0.14),
            life: (0.25, 0.8),
            gravity: 5.0,
            drag: 1.6,
        });
    }
    for (at, radius, color) in fx.rings {
        // Ring: particles spawned outward along the rim.
        let count = (radius * 10.0) as u32;
        frame.particle_spawns.push(ParticleSpawn {
            pos: Vec3::new(at.x, 0.4, at.y),
            count: count.clamp(16, 200),
            vel: Vec3::ZERO,
            spread: radius * 1.4,
            color_from: color.extend(0.9),
            color_to: color.extend(0.0),
            size: (0.06, 0.16),
            life: (0.2, 0.5),
            gravity: 0.0,
            drag: 4.0,
        });
    }
}
