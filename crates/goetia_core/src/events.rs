//! Double-buffered event channel. Events written during tick N are readable
//! during tick N (same-tick systems later in the schedule) and tick N+1, then
//! dropped. Store instances as World resources.

pub struct Events<T> {
    front: Vec<T>,
    back: Vec<T>,
}

impl<T> Default for Events<T> {
    fn default() -> Self {
        Events {
            front: Vec::new(),
            back: Vec::new(),
        }
    }
}

impl<T> Events<T> {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn send(&mut self, ev: T) {
        self.front.push(ev);
    }

    /// Everything visible right now: this tick's events plus last tick's.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.back.iter().chain(self.front.iter())
    }

    /// Drain current events (consumes this tick's buffer only).
    pub fn drain_current(&mut self) -> std::vec::Drain<'_, T> {
        self.front.drain(..)
    }

    pub fn len(&self) -> usize {
        self.front.len() + self.back.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Call once per tick (end of tick): ages front->back, drops old back.
    pub fn update(&mut self) {
        std::mem::swap(&mut self.front, &mut self.back);
        self.front.clear();
    }
}
