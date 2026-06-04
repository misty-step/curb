use std::sync::{Mutex, TryLockError};

use crate::runtime::readiness::SnapshotCacheStatus;
use crate::service::{self, Snapshot};

pub(crate) struct SnapshotCache {
    inner: Mutex<Option<Snapshot>>,
}

impl SnapshotCache {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(None),
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
        Ok(snapshot)
    }

    pub(crate) fn clear(&self) {
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
                TryLockError::WouldBlock => SnapshotCacheStatus::Busy,
            })
    }

    #[cfg(test)]
    pub(crate) fn lock_for_test(&self) -> std::sync::MutexGuard<'_, Option<Snapshot>> {
        self.inner.lock().expect("runtime cache mutex poisoned")
    }
}
