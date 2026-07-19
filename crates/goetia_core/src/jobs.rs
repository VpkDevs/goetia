//! Work-stealing job pool. Worker threads own local deques and steal from the
//! global injector and from each other; `scope` blocks the caller (who also
//! works) until every job submitted in that scope completes.

use crossbeam_deque::{Injector, Stealer, Worker};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

type Job = Box<dyn FnOnce() + Send>;

struct Shared {
    injector: Injector<Job>,
    stealers: Vec<Stealer<Job>>,
    pending: AtomicUsize,
    shutdown: AtomicBool,
    idle: Mutex<()>,
    wake: Condvar,
}

pub struct JobPool {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl JobPool {
    /// `threads = 0` means "hardware parallelism minus one" (the caller thread
    /// participates during `scope`).
    pub fn new(threads: usize) -> Self {
        let n = if threads == 0 {
            std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4).saturating_sub(1).max(1)
        } else {
            threads
        };
        let workers: Vec<Worker<Job>> = (0..n).map(|_| Worker::new_fifo()).collect();
        let stealers = workers.iter().map(|w| w.stealer()).collect();
        let shared = Arc::new(Shared {
            injector: Injector::new(),
            stealers,
            pending: AtomicUsize::new(0),
            shutdown: AtomicBool::new(false),
            idle: Mutex::new(()),
            wake: Condvar::new(),
        });
        let threads = workers
            .into_iter()
            .enumerate()
            .map(|(i, w)| {
                let s = shared.clone();
                std::thread::Builder::new()
                    .name(format!("goetia-worker-{i}"))
                    .spawn(move || worker_loop(w, s))
                    .expect("spawn worker")
            })
            .collect();
        JobPool { shared, threads }
    }

    pub fn worker_count(&self) -> usize {
        self.threads.len()
    }

    /// Run all closures, potentially in parallel; returns when all are done.
    /// The calling thread participates in execution.
    pub fn scope(&self, jobs: Vec<Box<dyn FnOnce() + Send + '_>>) {
        if jobs.is_empty() {
            return;
        }
        if jobs.len() == 1 {
            let mut jobs = jobs;
            (jobs.pop().unwrap())();
            return;
        }
        let count = jobs.len();
        let done = AtomicUsize::new(0);
        let done_ref: &AtomicUsize = &done;
        self.shared.pending.fetch_add(count, Ordering::SeqCst);
        for job in jobs {
            // Safety: we block in this function until `done == count`, so the
            // 'scope lifetime outlives every job. Erase the lifetime to 'static
            // for storage in the deque.
            let job: Box<dyn FnOnce() + Send + 'static> = unsafe { std::mem::transmute(job) };
            let done_ptr: &'static AtomicUsize = unsafe { std::mem::transmute(done_ref) };
            self.shared.injector.push(Box::new(move || {
                job();
                done_ptr.fetch_add(1, Ordering::SeqCst);
            }));
        }
        // Wake sleeping workers.
        let _g = self.shared.idle.lock().unwrap();
        drop(_g);
        self.shared.wake.notify_all();
        // Caller works too.
        while done.load(Ordering::SeqCst) < count {
            match self.shared.injector.steal() {
                crossbeam_deque::Steal::Success(job) => {
                    job();
                    self.shared.pending.fetch_sub(1, Ordering::SeqCst);
                }
                _ => std::hint::spin_loop(),
            }
        }
    }
}

fn worker_loop(local: Worker<Job>, shared: Arc<Shared>) {
    loop {
        if shared.shutdown.load(Ordering::SeqCst) {
            return;
        }
        let job = local.pop().or_else(|| {
            std::iter::repeat_with(|| {
                shared
                    .injector
                    .steal_batch_and_pop(&local)
                    .or_else(|| shared.stealers.iter().map(|s| s.steal()).collect())
            })
            .find(|s| !s.is_retry())
            .and_then(|s| s.success())
        });
        match job {
            Some(job) => {
                job();
                shared.pending.fetch_sub(1, Ordering::SeqCst);
            }
            None => {
                let g = shared.idle.lock().unwrap();
                if shared.pending.load(Ordering::SeqCst) == 0
                    && !shared.shutdown.load(Ordering::SeqCst)
                {
                    let _ = shared
                        .wake
                        .wait_timeout(g, std::time::Duration::from_millis(2))
                        .unwrap();
                }
            }
        }
    }
}

impl Drop for JobPool {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        self.shared.wake.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn scope_runs_all_jobs() {
        let pool = JobPool::new(4);
        let sum = AtomicU64::new(0);
        let jobs: Vec<Box<dyn FnOnce() + Send + '_>> = (1..=100u64)
            .map(|i| {
                let s = &sum;
                Box::new(move || {
                    s.fetch_add(i, Ordering::SeqCst);
                }) as Box<dyn FnOnce() + Send + '_>
            })
            .collect();
        pool.scope(jobs);
        assert_eq!(sum.load(Ordering::SeqCst), 5050);
    }
}
