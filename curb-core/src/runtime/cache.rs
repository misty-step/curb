use std::sync::{
    Mutex, TryLockError,
    atomic::{AtomicBool, Ordering},
};

use crate::runtime::readiness::SnapshotCacheStatus;
use crate::service::{self, Snapshot};

pub(crate) struct SnapshotCache {
    inner: Mutex<Option<Snapshot>>,
    initialized: AtomicBool,
}

impl SnapshotCache {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            initialized: AtomicBool::new(false),
        }
    }

    pub(crate) fn get(&self) -> Option<Snapshot> {
        self.inner
            .lock()
            .expect("runtime cache mutex poisoned")
            .clone()
    }

    pub(crate) fn refresh<E>(
        &self,
        build: impl FnOnce() -> Result<Snapshot, E>,
    ) -> Result<Snapshot, E> {
        let mut cache = self.inner.lock().expect("runtime cache mutex poisoned");
        let snapshot = service::annotate_overview_delta(cache.as_ref(), build()?);
        *cache = Some(snapshot.clone());
        self.initialized.store(true, Ordering::Release);
        Ok(snapshot)
    }

    pub(crate) fn clear(&self) {
        self.initialized.store(false, Ordering::Release);
        *self.inner.lock().expect("runtime cache mutex poisoned") = None;
    }

    pub(crate) fn status(&self) -> SnapshotCacheStatus {
        self.inner
            .try_lock()
            .map(|cache| {
                if cache.is_some() {
                    SnapshotCacheStatus::Ready
                } else {
                    SnapshotCacheStatus::Unavailable
                }
            })
            .unwrap_or_else(|error| match error {
                TryLockError::Poisoned(_) => SnapshotCacheStatus::Poisoned,
                TryLockError::WouldBlock => {
                    if self.initialized.load(Ordering::Acquire) {
                        SnapshotCacheStatus::RefreshingCached
                    } else {
                        SnapshotCacheStatus::Busy
                    }
                }
            })
    }

    #[cfg(test)]
    pub(crate) fn lock_for_test(&self) -> std::sync::MutexGuard<'_, Option<Snapshot>> {
        self.inner.lock().expect("runtime cache mutex poisoned")
    }
}
