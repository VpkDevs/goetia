//! System scheduler. Systems declare component/resource read & write sets;
//! consecutive non-conflicting systems are batched and run in parallel on the
//! job pool. Registration order is preserved as the logical execution order,
//! which keeps the simulation deterministic regardless of thread timing:
//! parallelism only ever happens between systems whose data sets are disjoint.

use crate::ecs::World;
use crate::jobs::JobPool;
use std::any::TypeId;

pub struct SystemDef {
    pub name: &'static str,
    pub reads: Vec<TypeId>,
    pub writes: Vec<TypeId>,
    /// Exclusive systems get the whole world (structural changes allowed) and
    /// always run alone.
    pub exclusive: bool,
    pub run: Box<dyn Fn(&mut World) + Send + Sync>,
}

impl SystemDef {
    pub fn exclusive(name: &'static str, run: impl Fn(&mut World) + Send + Sync + 'static) -> Self {
        SystemDef {
            name,
            reads: vec![],
            writes: vec![],
            exclusive: true,
            run: Box::new(run),
        }
    }

    pub fn new(name: &'static str, run: impl Fn(&mut World) + Send + Sync + 'static) -> Self {
        SystemDef {
            name,
            reads: vec![],
            writes: vec![],
            exclusive: false,
            run: Box::new(run),
        }
    }

    pub fn reads<T: 'static>(mut self) -> Self {
        self.reads.push(TypeId::of::<T>());
        self
    }
    pub fn writes<T: 'static>(mut self) -> Self {
        self.writes.push(TypeId::of::<T>());
        self
    }
}

fn conflicts(a: &SystemDef, b: &SystemDef) -> bool {
    if a.exclusive || b.exclusive {
        return true;
    }
    let w_r = a
        .writes
        .iter()
        .any(|t| b.reads.contains(t) || b.writes.contains(t));
    let r_w = b.writes.iter().any(|t| a.reads.contains(t));
    w_r || r_w
}

#[derive(Default)]
pub struct Schedule {
    systems: Vec<SystemDef>,
    /// Precomputed batches (ranges into `systems`), rebuilt when systems change.
    batches: Vec<Vec<usize>>,
    dirty: bool,
    /// Per-system last-run duration (µs), for the debug overlay.
    pub timings: Vec<(&'static str, u32)>,
}

struct WorldPtr(*mut World);
unsafe impl Send for WorldPtr {}
unsafe impl Sync for WorldPtr {}

impl Schedule {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, sys: SystemDef) -> &mut Self {
        self.systems.push(sys);
        self.dirty = true;
        self
    }

    fn rebuild(&mut self) {
        self.batches.clear();
        let mut current: Vec<usize> = Vec::new();
        for i in 0..self.systems.len() {
            let ok = !current
                .iter()
                .any(|&j| conflicts(&self.systems[j], &self.systems[i]));
            if ok && !self.systems[i].exclusive {
                current.push(i);
            } else {
                if !current.is_empty() {
                    self.batches.push(std::mem::take(&mut current));
                }
                current.push(i);
                if self.systems[i].exclusive {
                    self.batches.push(std::mem::take(&mut current));
                }
            }
        }
        if !current.is_empty() {
            self.batches.push(current);
        }
        self.timings = self.systems.iter().map(|s| (s.name, 0)).collect();
        self.dirty = false;
    }

    pub fn run(&mut self, world: &mut World, pool: &JobPool) {
        if self.dirty {
            self.rebuild();
        }
        for batch in &self.batches {
            if batch.len() == 1 {
                let i = batch[0];
                let t = std::time::Instant::now();
                (self.systems[i].run)(world);
                self.timings[i].1 = t.elapsed().as_micros() as u32;
            } else {
                let wp = WorldPtr(world as *mut World);
                let wp_ref = &wp;
                let systems = &self.systems;
                let timings: Vec<std::sync::atomic::AtomicU32> = batch
                    .iter()
                    .map(|_| std::sync::atomic::AtomicU32::new(0))
                    .collect();
                let jobs: Vec<Box<dyn FnOnce() + Send + '_>> = batch
                    .iter()
                    .enumerate()
                    .map(|(k, &i)| {
                        let tk = &timings[k];
                        Box::new(move || {
                            let t = std::time::Instant::now();
                            // Safety: all systems in a batch have disjoint
                            // write sets (checked in rebuild), so concurrent
                            // &mut World access never aliases component data.
                            let w = unsafe { &mut *wp_ref.0 };
                            (systems[i].run)(w);
                            tk.store(
                                t.elapsed().as_micros() as u32,
                                std::sync::atomic::Ordering::Relaxed,
                            );
                        }) as Box<dyn FnOnce() + Send + '_>
                    })
                    .collect();
                pool.scope(jobs);
                for (k, &i) in batch.iter().enumerate() {
                    self.timings[i].1 = timings[k].load(std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
    }

    pub fn batch_count(&mut self) -> usize {
        if self.dirty {
            self.rebuild();
        }
        self.batches.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct A(u64);
    struct B(u64);

    #[test]
    fn parallel_batches_disjoint() {
        let mut world = World::new();
        for i in 0..100u64 {
            world.spawn((A(i), B(i)));
        }
        let pool = JobPool::new(2);
        let mut sched = Schedule::new();
        sched.add(
            SystemDef::new("bump_a", |w| w.each::<(&mut A,)>(|_, (a,)| a.0 += 1)).writes::<A>(),
        );
        sched.add(
            SystemDef::new("bump_b", |w| w.each::<(&mut B,)>(|_, (b,)| b.0 += 2)).writes::<B>(),
        );
        sched.add(
            SystemDef::new("sum", |w| {
                let mut s = 0;
                w.each::<(&A, &B)>(|_, (a, b)| s += a.0 + b.0);
                assert_eq!(s, (0..100).map(|i| i + 1 + i + 2).sum::<u64>());
            })
            .reads::<A>()
            .reads::<B>(),
        );
        assert_eq!(sched.batch_count(), 2); // {bump_a, bump_b} then {sum}
        sched.run(&mut world, &pool);
    }
}
