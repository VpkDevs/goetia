//! Keyboard/mouse state with per-frame edge detection.

use glam::Vec2;
use std::collections::HashSet;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

#[derive(Default)]
pub struct Input {
    down: HashSet<KeyCode>,
    pressed: HashSet<KeyCode>,
    released: HashSet<KeyCode>,
    mouse_down: [bool; 3],
    mouse_pressed: [bool; 3],
    pub mouse_pos: Vec2,
    pub scroll: f32,
}

fn btn_index(b: MouseButton) -> Option<usize> {
    match b {
        MouseButton::Left => Some(0),
        MouseButton::Right => Some(1),
        MouseButton::Middle => Some(2),
        _ => None,
    }
}

impl Input {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn key_down(&self, k: KeyCode) -> bool {
        self.down.contains(&k)
    }
    /// True only on the frame the key went down.
    pub fn key_pressed(&self, k: KeyCode) -> bool {
        self.pressed.contains(&k)
    }
    pub fn key_released(&self, k: KeyCode) -> bool {
        self.released.contains(&k)
    }
    pub fn mouse_down(&self, b: usize) -> bool {
        self.mouse_down.get(b).copied().unwrap_or(false)
    }
    pub fn mouse_pressed(&self, b: usize) -> bool {
        self.mouse_pressed.get(b).copied().unwrap_or(false)
    }

    /// Feed a winit event (the App does this).
    pub fn handle(&mut self, ev: &WindowEvent) {
        match ev {
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            if self.down.insert(code) {
                                self.pressed.insert(code);
                            }
                        }
                        ElementState::Released => {
                            self.down.remove(&code);
                            self.released.insert(code);
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = Vec2::new(position.x as f32, position.y as f32);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(i) = btn_index(*button) {
                    match state {
                        ElementState::Pressed => {
                            if !self.mouse_down[i] {
                                self.mouse_pressed[i] = true;
                            }
                            self.mouse_down[i] = true;
                        }
                        ElementState::Released => self.mouse_down[i] = false,
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.scroll += match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
            }
            _ => {}
        }
    }

    /// Clear per-frame edges; the App calls this at end of frame.
    pub fn end_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
        self.mouse_pressed = [false; 3];
        self.scroll = 0.0;
    }
}
