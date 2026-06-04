use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::runtime::RuntimeError;

pub struct WatcherHandle {
    shutdown: Arc<WatcherShutdown>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct WatcherShutdown {
    stopped: Mutex<bool>,
    changed: Condvar,
}

pub(crate) fn start_usage_watcher<T>(
    interval: impl Fn() -> Duration + Send + 'static,
    tick: impl Fn(DateTime<Utc>) -> Result<T, RuntimeError> + Send + 'static,
    mut observe: impl FnMut(Result<&T, &RuntimeError>, Duration) + Send + 'static,
) -> WatcherHandle
where
    T: Send + 'static,
{
    let shutdown = Arc::new(WatcherShutdown::default());
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::spawn(move || {
        loop {
            if thread_shutdown.wait_timeout(interval()) {
                return;
            }
            let started = Instant::now();
            match tick(Utc::now()) {
                Ok(snapshot) => observe(Ok(&snapshot), started.elapsed()),
                Err(error) => {
                    observe(Err(&error), started.elapsed());
                    eprintln!("curb: usage scan failed: {error:#}");
                }
            }
        }
    });
    WatcherHandle {
        shutdown,
        thread: Some(thread),
    }
}

impl WatcherHandle {
    pub fn request_shutdown(&self) {
        let mut stopped = self
            .shutdown
            .stopped
            .lock()
            .expect("watcher mutex poisoned");
        *stopped = true;
        self.shutdown.changed.notify_all();
    }

    pub fn join(mut self) -> thread::Result<()> {
        self.request_shutdown();
        self.thread.take().map_or(Ok(()), JoinHandle::join)
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

impl WatcherShutdown {
    fn wait_timeout(&self, interval: Duration) -> bool {
        let stopped = self.stopped.lock().expect("watcher mutex poisoned");
        if *stopped {
            return true;
        }
        let (stopped, _) = self
            .changed
            .wait_timeout(stopped, interval)
            .expect("watcher condvar poisoned");
        *stopped
    }
}
