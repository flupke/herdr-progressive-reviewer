use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use crate::{Request, Results, ripgrep::Engine};

/// A single background search worker. New requests cancel and replace old work.
pub struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    changed: Condvar,
    generation: AtomicU64,
}

#[derive(Default)]
struct Pending {
    request: Option<(u64, Request)>,
    stopped: bool,
}

impl Worker {
    pub fn start(deliver: impl Fn(Results) + Send + 'static) -> Self {
        let shared = Arc::new(Shared::default());
        let worker_shared = Arc::clone(&shared);
        let thread = thread::spawn(move || worker_shared.run(deliver));
        Self {
            shared,
            thread: Some(thread),
        }
    }

    /// Submit the newest query, or cancel pending work with `None`.
    ///
    /// # Panics
    /// Panics if another thread poisoned the search queue lock.
    pub fn submit(&self, request: Option<Request>) {
        let mut pending = self.shared.pending.lock().expect("search queue lock");
        let generation = self
            .shared
            .generation
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        pending.request = request.map(|request| (generation, request));
        self.shared.changed.notify_one();
    }
}

impl Shared {
    fn run(&self, deliver: impl Fn(Results)) {
        let mut engine = Engine::default();
        loop {
            let mut pending = self.pending.lock().expect("search queue lock");
            while pending.request.is_none() && !pending.stopped {
                pending = self.changed.wait(pending).expect("search queue lock");
            }
            if pending.stopped {
                return;
            }
            let (generation, request) = pending.request.take().expect("pending search");
            drop(pending);
            let cancelled = || self.generation.load(Ordering::Relaxed) != generation;
            if let Some(results) = engine.search(&request, cancelled) {
                deliver(results);
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        {
            let mut pending = self.shared.pending.lock().expect("search queue lock");
            pending.stopped = true;
            pending.request = None;
            self.shared.generation.fetch_add(1, Ordering::Relaxed);
            self.shared.changed.notify_one();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
