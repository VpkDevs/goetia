//! App scaffolding: fixed-tick sim / interpolated render loop, pause-resilient
//! timing, headless mode for CI, bench reporting.

use crate::input::Input;
use crate::overlay::Overlay;
use glam::Vec2;
use goetia_audio::AudioEngine;
use goetia_combat::TriggerBus;
use goetia_core::time::FIXED_DT;
use goetia_core::{CommandBuffer, GameClock, JobPool, PcgStreams, Schedule, World};
use goetia_render::{CameraRig, FrameSubmit, Renderer};
use std::sync::Arc;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::KeyCode;
use winit::window::WindowBuilder;

/// Everything the game touches every tick/frame. One bag, no globals.
pub struct Engine {
    pub world: World,
    pub schedule: Schedule,
    pub jobs: JobPool,
    pub clock: GameClock,
    /// Named deterministic RNG streams (`layout`, `loot`, `packs`, …).
    pub streams: PcgStreams,
    pub triggers: TriggerBus,
    pub audio: AudioEngine,
    pub input: Input,
    pub camera: CameraRig,
    /// Deferred structural changes; applied after each fixed_update.
    pub commands: CommandBuffer,
    pub overlay: Overlay,
    pub floaters: crate::floaters::DamageNumbers,
    /// Window size in pixels (1,1 when headless).
    pub viewport: Vec2,
    /// Set true to exit the app loop.
    pub quit: bool,
}

impl Engine {
    pub fn new(master_seed: u64, threads: usize, headless: bool) -> Engine {
        Engine {
            world: World::new(),
            schedule: Schedule::new(),
            jobs: JobPool::new(threads),
            clock: GameClock::new(),
            streams: PcgStreams::new(master_seed),
            triggers: TriggerBus::default(),
            audio: if headless {
                // Skip device probing in CI.
                AudioEngine::disabled()
            } else {
                AudioEngine::new()
            },
            input: Input::new(),
            camera: CameraRig::new(),
            commands: CommandBuffer::new(),
            overlay: Overlay::new(),
            floaters: crate::floaters::DamageNumbers::new(),
            viewport: Vec2::ONE,
            quit: false,
        }
    }

    /// Run the registered system schedule (parallel where access sets allow).
    pub fn run_schedule(&mut self) {
        self.schedule.run(&mut self.world, &self.jobs);
    }

    /// Freeze sim time for impact frames.
    pub fn hitstop(&mut self, seconds: f64) {
        self.clock.hitstop(seconds);
    }

    /// Screenshake trauma (0..1-ish per hit).
    pub fn shake(&mut self, trauma: f32) {
        self.camera.add_trauma(trauma);
    }

    /// Cursor position on the ground plane (y=0).
    pub fn mouse_ground(&self) -> glam::Vec3 {
        let aspect = self.viewport.x / self.viewport.y.max(1.0);
        self.camera
            .screen_to_ground(self.input.mouse_pos, self.viewport, aspect)
    }
}

pub trait Game {
    /// Called once. `gfx` is None when running headless (register meshes only
    /// when it's Some).
    fn init(&mut self, eng: &mut Engine, gfx: Option<&mut Renderer>);
    /// Called at exactly 60 Hz sim time. All gameplay mutation lives here.
    fn fixed_update(&mut self, eng: &mut Engine);
    /// Called once per rendered frame; fill `frame` with instances, lights,
    /// particles, UI. `alpha` interpolates between the last two ticks.
    fn render_extract(&mut self, eng: &mut Engine, frame: &mut FrameSubmit, alpha: f32);
    /// Extra UI after render_extract (optional).
    fn ui(&mut self, _eng: &mut Engine, _ui: &mut goetia_render::UiBatch) {}
    /// Raw window events (optional).
    fn on_event(&mut self, _eng: &mut Engine, _ev: &WindowEvent) {}
}

pub struct AppConfig {
    pub title: String,
    pub size: (u32, u32),
    pub vsync: bool,
    /// Exit after N frames and print a bench report (sandbox/CI use).
    pub max_frames: Option<u64>,
    pub master_seed: u64,
    /// 0 = auto.
    pub threads: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            title: "GOETIA".into(),
            size: (1600, 900),
            vsync: true,
            max_frames: None,
            master_seed: 0x6047_1A00_D3E0_0666, // "goetia"
            threads: 0,
        }
    }
}

pub struct App;

impl App {
    /// Windowed main loop. Blocks until quit.
    pub fn run(config: AppConfig, mut game: impl Game + 'static) -> Result<(), String> {
        let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
        event_loop.set_control_flow(ControlFlow::Poll);
        let window = Arc::new(
            WindowBuilder::new()
                .with_title(&config.title)
                .with_inner_size(winit::dpi::LogicalSize::new(
                    config.size.0 as f64,
                    config.size.1 as f64,
                ))
                .build(&event_loop)
                .map_err(|e| e.to_string())?,
        );
        let mut renderer = Renderer::new(window.clone(), config.vsync);
        let mut eng = Engine::new(config.master_seed, config.threads, false);
        eng.viewport = Vec2::new(renderer.size.0 as f32, renderer.size.1 as f32);
        game.init(&mut eng, Some(&mut renderer));
        eng.commands_apply();

        let mut last = std::time::Instant::now();
        let mut frame_count: u64 = 0;
        let max_frames = config.max_frames;

        event_loop
            .run(move |event, elwt| match event {
                Event::AboutToWait => window.request_redraw(),
                Event::WindowEvent { event, .. } => {
                    eng.input.handle(&event);
                    game.on_event(&mut eng, &event);
                    match event {
                        WindowEvent::CloseRequested => elwt.exit(),
                        WindowEvent::Resized(sz) => {
                            renderer.resize(sz.width, sz.height);
                            eng.viewport = Vec2::new(sz.width as f32, sz.height as f32);
                        }
                        WindowEvent::RedrawRequested => {
                            let now = std::time::Instant::now();
                            let dt = (now - last).as_secs_f64();
                            last = now;

                            if eng.input.key_pressed(KeyCode::F1) {
                                eng.overlay.enabled = !eng.overlay.enabled;
                            }

                            let in_hitstop = eng.clock.hitstop > 0.0;
                            let ticks = eng.clock.advance(dt);
                            for _ in 0..ticks {
                                game.fixed_update(&mut eng);
                                eng.commands_apply();
                                eng.clock.tick += 1;
                            }

                            eng.camera.update(dt as f32);
                            eng.floaters.update(dt as f32);

                            let mut frame = FrameSubmit {
                                // Hitstop freezes particles too, so the whole
                                // frame reads as one held beat.
                                particle_dt: if in_hitstop {
                                    0.0
                                } else {
                                    (dt * eng.clock.time_scale) as f32
                                },
                                ..Default::default()
                            };
                            let alpha = eng.clock.alpha;
                            game.render_extract(&mut eng, &mut frame, alpha);
                            game.ui(&mut eng, &mut frame.ui);
                            let cam_snapshot = eng.viewport;
                            eng.floaters.draw(&mut frame.ui, &eng.camera, cam_snapshot);

                            eng.overlay.push_frame(dt as f32);
                            let ws = (eng.world.entity_count(), eng.world.archetype_stats());
                            let tstats = eng.triggers.stats;
                            let timings: Vec<_> = eng.schedule.timings.clone();
                            eng.overlay.draw(
                                &mut frame.ui,
                                ws,
                                Some(&tstats),
                                &renderer.stats,
                                &timings,
                                eng.clock.tick,
                            );

                            renderer.render(&eng.camera, &mut frame);
                            eng.input.end_frame();

                            frame_count += 1;
                            if eng.quit || max_frames.map(|m| frame_count >= m).unwrap_or(false) {
                                Self::print_bench(&eng, frame_count);
                                elwt.exit();
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            })
            .map_err(|e| e.to_string())
    }

    fn print_bench(eng: &Engine, frames: u64) {
        let (avg, max, worst) = eng.overlay.stats();
        println!("--- goetia bench report ---");
        println!("frames:        {frames}");
        println!(
            "avg frame:     {avg:.2} ms ({:.0} fps)",
            1000.0 / avg.max(0.001)
        );
        println!("window max:    {max:.2} ms");
        println!("worst ever:    {worst:.2} ms");
        println!(
            "frames >20ms:  {} of {}",
            eng.overlay.frames_over_20ms, eng.overlay.total_frames
        );
    }

    /// Headless fixed-tick run for CI / determinism tests. Returns the engine
    /// so callers can hash world state.
    pub fn run_headless(mut game: impl Game, master_seed: u64, ticks: u64) -> Engine {
        let mut eng = Engine::new(master_seed, 1, true);
        game.init(&mut eng, None);
        eng.commands_apply();
        for _ in 0..ticks {
            game.fixed_update(&mut eng);
            eng.commands_apply();
            eng.clock.tick += 1;
        }
        eng
    }
}

impl Engine {
    fn commands_apply(&mut self) {
        if !self.commands.is_empty() {
            let mut cmds = std::mem::take(&mut self.commands);
            cmds.apply(&mut self.world);
            self.commands = cmds;
        }
    }
}

const _: () = {
    // FIXED_DT is part of the frozen contract; keep it referenced.
    let _ = FIXED_DT;
};
