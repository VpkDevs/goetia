//! Engine debug overlay: frame-time graph, entity/archetype counts, trigger
//! bus stats, particle count, system timings, plus game-provided lines.
//! Toggle with F1. This overlay is how you learn to trust the engine.

use glam::{Vec2, Vec4};
use goetia_render::{RenderStats, UiBatch};

pub struct Overlay {
    pub enabled: bool,
    /// Ring of recent frame times (seconds).
    frames: Vec<f32>,
    cursor: usize,
    /// Extra lines pushed by the game each frame.
    pub lines: Vec<String>,
    pub frames_over_20ms: u64,
    pub total_frames: u64,
    worst: f32,
}

const RING: usize = 240;

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

impl Overlay {
    pub fn new() -> Self {
        Overlay {
            enabled: true,
            frames: vec![0.0; RING],
            cursor: 0,
            lines: Vec::new(),
            frames_over_20ms: 0,
            total_frames: 0,
            worst: 0.0,
        }
    }

    pub fn push_frame(&mut self, dt: f32) {
        self.frames[self.cursor] = dt;
        self.cursor = (self.cursor + 1) % RING;
        self.total_frames += 1;
        // First seconds compile pipelines / warm caches; excluding them keeps
        // the >20ms gate about steady-state, which is what the target means.
        if self.total_frames > 60 {
            if dt > 0.020 {
                self.frames_over_20ms += 1;
            }
            self.worst = self.worst.max(dt);
        }
    }

    pub fn stats(&self) -> (f32, f32, f32) {
        // (avg, max in window, worst ever) in ms
        let n = self.frames.iter().filter(|f| **f > 0.0).count().max(1);
        let sum: f32 = self.frames.iter().sum();
        let max = self.frames.iter().cloned().fold(0.0, f32::max);
        (sum / n as f32 * 1000.0, max * 1000.0, self.worst * 1000.0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        ui: &mut UiBatch,
        world_stats: (usize, Vec<(usize, usize)>),
        trigger_stats: Option<&goetia_combat::TriggerStats>,
        render_stats: &RenderStats,
        timings: &[(&'static str, u32)],
        tick: u64,
    ) {
        if !self.enabled {
            self.lines.clear();
            return;
        }
        let fg = Vec4::new(0.85, 0.95, 0.85, 1.0);
        let dim = Vec4::new(0.6, 0.65, 0.7, 1.0);
        let warn = Vec4::new(1.0, 0.45, 0.2, 1.0);
        let bg = Vec4::new(0.0, 0.0, 0.0, 0.55);

        let (avg, max, worst) = self.stats();
        let x = 8.0;
        let mut y = 8.0;
        let line = 18.0;

        ui.rect(Vec2::new(4.0, 4.0), Vec2::new(360.0, 460.0), bg);

        let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
        ui.text(
            Vec2::new(x, y),
            2.0,
            if max > 20.0 { warn } else { fg },
            &format!("{fps:5.0} FPS  {avg:5.2}MS AVG  {max:5.2}MS MAX"),
        );
        y += line * 1.4;

        // Frame graph: 120 bars, 20ms = full height red line.
        let gw = 340.0;
        let gh = 48.0;
        ui.rect(
            Vec2::new(x, y),
            Vec2::new(gw, gh),
            Vec4::new(0.0, 0.0, 0.0, 0.5),
        );
        let budget_y = y + gh - (16.67 / 33.3) * gh;
        ui.rect(
            Vec2::new(x, budget_y),
            Vec2::new(gw, 1.0),
            Vec4::new(0.3, 0.9, 0.4, 0.7),
        );
        let bars = 120usize;
        let bw = gw / bars as f32;
        for i in 0..bars {
            let idx = (self.cursor + RING - bars + i) % RING;
            let ms = self.frames[idx] * 1000.0;
            let h = (ms / 33.3 * gh).min(gh);
            let c = if ms > 20.0 {
                warn
            } else if ms > 16.7 {
                Vec4::new(0.95, 0.8, 0.2, 0.9)
            } else {
                Vec4::new(0.4, 0.8, 0.5, 0.8)
            };
            ui.rect(
                Vec2::new(x + i as f32 * bw, y + gh - h),
                Vec2::new(bw - 1.0, h),
                c,
            );
        }
        y += gh + 10.0;

        let mut text = |s: String, c: Vec4, y: &mut f32| {
            ui.text(Vec2::new(x, *y), 1.5, c, &s);
            *y += line;
        };

        text(
            format!(
                "TICK {tick}   WORST {worst:5.2}MS   >20MS: {}",
                self.frames_over_20ms
            ),
            dim,
            &mut y,
        );
        let (entities, archs) = &world_stats;
        text(
            format!("ENTITIES {entities}   ARCHETYPES {}", archs.len()),
            fg,
            &mut y,
        );
        let mut arch_line = String::from("  ");
        for (comps, count) in archs.iter().take(6) {
            arch_line.push_str(&format!("[{comps}C:{count}] "));
        }
        text(arch_line, dim, &mut y);
        text(
            format!(
                "DRAWS {}  INSTANCES {}  LIGHTS {}  PARTICLES {}",
                render_stats.draw_calls,
                render_stats.instances,
                render_stats.lights,
                render_stats.particles_alive_estimate
            ),
            fg,
            &mut y,
        );
        if let Some(t) = trigger_stats {
            text(
                format!(
                    "TRIGGERS: {}/TICK  DEPTH {}  PEND->DROP {}",
                    t.last_tick_processed, t.max_depth_seen, t.dropped_budget
                ),
                if t.dropped_budget > 0 { warn } else { fg },
                &mut y,
            );
            text(
                format!(
                    "  EMIT {}  PROC {}  BUDGET-HITS {}",
                    t.emitted, t.processed, t.budget_hit_ticks
                ),
                dim,
                &mut y,
            );
        }
        for (name, us) in timings.iter().take(8) {
            text(
                format!("  {name:<18} {:6.2}MS", *us as f32 / 1000.0),
                dim,
                &mut y,
            );
        }
        for l in &self.lines {
            text(l.clone(), fg, &mut y);
        }
        self.lines.clear();
    }
}
