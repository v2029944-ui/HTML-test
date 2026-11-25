//! Cancellation, progress and commit coordination for management operations.
use super::err;
use rusqlite::{Connection, Transaction};
use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

#[derive(Default)]
pub struct ManagementJob {
    state: Mutex<u8>,
    cancelled: Arc<AtomicBool>,
    records: AtomicU64,
    interrupt: Mutex<Option<rusqlite::InterruptHandle>>,
}
impl ManagementJob {
    pub fn new() -> Self {
        Self::default()
    }
    pub(super) fn with_cancellation(cancelled: Arc<AtomicBool>) -> Self {
        Self {
            cancelled,
            ..Self::default()
        }
    }
    pub fn records_processed(&self) -> u64 {
        self.records.load(Ordering::Relaxed)
    }
    pub(super) fn record(&self) {
        self.records.fetch_add(1, Ordering::Relaxed);
    }
    pub fn cancel(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if *state == 2 {
            return false;
        }
        *state = 1;
        self.cancelled.store(true, Ordering::Release);
        if let Some(h) = self
            .interrupt
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            h.interrupt();
        }
        true
    }
    pub(super) fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Management cancelled before commit".into())
        } else {
            Ok(())
        }
    }
    pub(super) fn run<T>(&self, c: &Connection, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        self.check()?;
        *self.interrupt.lock().map_err(err)? = Some(c.get_interrupt_handle());
        struct Guard<'a>(&'a ManagementJob);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                *self.0.interrupt.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
        }
        let _guard = Guard(self);
        self.check()?;
        f()
    }
    pub(super) fn finish(&self, tx: Transaction<'_>) -> Result<(), String> {
        self.check()?;
        let mut state = self.state.lock().map_err(err)?;
        if *state != 0 {
            return Err("Management cancelled or already committed".into());
        }
        tx.commit().map_err(err)?;
        *state = 2;
        Ok(())
    }
}

pub(super) struct JobRead<'a, R> {
    pub inner: R,
    pub job: &'a ManagementJob,
}
impl<R: Read> Read for JobRead<'_, R> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.job.check().map_err(std::io::Error::other)?;
        self.inner.read(b)
    }
}
