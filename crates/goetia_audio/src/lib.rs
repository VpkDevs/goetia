//! goetia_audio — thin rodio wrapper: one-shots, loops, named buses.
//! Sounds are in-memory sample buffers; they can be decoded from OGG/WAV or
//! synthesized procedurally (the sandbox ships zero binary assets).
//!
//! Headless-safe: if no output device exists, everything becomes a no-op.

use rodio::buffer::SamplesBuffer;
use rodio::source::Source;
use rodio::{OutputStream, OutputStreamHandle, Sink};
use std::collections::HashMap;
use std::sync::Arc;

/// An immutable, shareable sound: mono or stereo f32 samples.
#[derive(Clone)]
pub struct Sound {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Arc<[f32]>,
}

impl Sound {
    /// Decode an OGG/WAV/FLAC/MP3 file fully into memory.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Sound, String> {
        let f = std::fs::File::open(path.as_ref()).map_err(|e| e.to_string())?;
        let dec = rodio::Decoder::new(std::io::BufReader::new(f)).map_err(|e| e.to_string())?;
        let channels = dec.channels();
        let sample_rate = dec.sample_rate();
        let samples: Vec<f32> = dec.convert_samples().collect();
        Ok(Sound { channels, sample_rate, samples: samples.into() })
    }

    /// Synthesize mono audio from a closure `f(t_seconds) -> sample`.
    pub fn synth(duration: f32, sample_rate: u32, f: impl Fn(f32) -> f32) -> Sound {
        let n = (duration * sample_rate as f32) as usize;
        let samples: Vec<f32> = (0..n).map(|i| f(i as f32 / sample_rate as f32)).collect();
        Sound { channels: 1, sample_rate, samples: samples.into() }
    }

    /// Percussive sine blip with exponential decay — projectile fire, UI ticks.
    pub fn blip(freq: f32, duration: f32) -> Sound {
        Sound::synth(duration, 44100, move |t| {
            let env = (-t * 18.0 / duration).exp();
            (t * freq * std::f32::consts::TAU).sin() * env * 0.8
        })
    }

    /// Filtered noise burst — impacts, explosions. `tone` in [0,1] darkens it.
    pub fn noise_burst(duration: f32, tone: f32) -> Sound {
        // Deterministic xorshift noise + one-pole lowpass.
        let mut state = 0x2545F4914F6CDD1Du64;
        let sr = 44100;
        let n = (duration * sr as f32) as usize;
        let mut lp = 0.0f32;
        let alpha = 0.02 + tone * 0.5;
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let white = ((state >> 40) as i32 as f32 / (1 << 23) as f32) - 1.0;
                lp += alpha * (white - lp);
                let t = i as f32 / n as f32;
                lp * (1.0 - t).powi(2) * 0.9
            })
            .collect();
        Sound { channels: 1, sample_rate: sr, samples: samples.into() }
    }

    fn buffer(&self) -> SamplesBuffer<f32> {
        SamplesBuffer::new(self.channels, self.sample_rate, self.samples.to_vec())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LoopHandle(usize);

pub struct AudioEngine {
    // None => headless / no device: all calls are silent no-ops.
    out: Option<(OutputStream, OutputStreamHandle)>,
    buses: HashMap<String, f32>,
    loops: Vec<Option<Sink>>,
    pub master: f32,
}

impl AudioEngine {
    pub fn new() -> AudioEngine {
        let out = match OutputStream::try_default() {
            Ok(pair) => Some(pair),
            Err(e) => {
                log::warn!("no audio device ({e}); audio disabled");
                None
            }
        };
        AudioEngine { out, buses: HashMap::new(), loops: Vec::new(), master: 1.0 }
    }

    /// Explicitly silent engine (headless/CI: skips device probing entirely).
    pub fn disabled() -> AudioEngine {
        AudioEngine { out: None, buses: HashMap::new(), loops: Vec::new(), master: 1.0 }
    }

    pub fn enabled(&self) -> bool {
        self.out.is_some()
    }

    /// Set a named bus volume (created on first use; default 1.0).
    pub fn set_bus(&mut self, bus: &str, volume: f32) {
        self.buses.insert(bus.to_string(), volume.clamp(0.0, 2.0));
    }

    fn bus_volume(&self, bus: &str) -> f32 {
        *self.buses.get(bus).unwrap_or(&1.0) * self.master
    }

    /// Fire-and-forget one-shot. `pitch` 1.0 = as-authored.
    pub fn play(&self, sound: &Sound, bus: &str, volume: f32, pitch: f32) {
        let Some((_, handle)) = &self.out else { return };
        let v = volume * self.bus_volume(bus);
        if v <= 0.0 {
            return;
        }
        let src = sound.buffer().speed(pitch.max(0.01)).amplify(v);
        if let Err(e) = handle.play_raw(src.convert_samples()) {
            log::warn!("play_raw failed: {e}");
        }
    }

    /// Start a looping sound; keep the handle to control or stop it.
    pub fn play_loop(&mut self, sound: &Sound, bus: &str, volume: f32) -> LoopHandle {
        let idx = self.loops.iter().position(|s| s.is_none()).unwrap_or(self.loops.len());
        let sink = self.out.as_ref().and_then(|(_, handle)| {
            let sink = Sink::try_new(handle).ok()?;
            sink.set_volume(volume * self.bus_volume(bus));
            sink.append(sound.buffer().repeat_infinite());
            Some(sink)
        });
        let slot = sink; // None when headless — handle still valid, no-ops.
        if idx == self.loops.len() {
            self.loops.push(slot);
        } else {
            self.loops[idx] = slot;
        }
        LoopHandle(idx)
    }

    pub fn set_loop_volume(&mut self, h: LoopHandle, volume: f32) {
        if let Some(Some(sink)) = self.loops.get(h.0) {
            sink.set_volume(volume.max(0.0));
        }
    }

    pub fn stop_loop(&mut self, h: LoopHandle) {
        if let Some(slot) = self.loops.get_mut(h.0) {
            if let Some(sink) = slot.take() {
                sink.stop();
            }
        }
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}
