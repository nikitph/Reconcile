//! Storage backend abstraction.
//!
//! Combines all 4 storage traits into a single backend that can
//! provide transaction control (begin/commit/rollback).

use crate::audit_log::AuditStore;
use crate::errors::KernelError;
use crate::event_log::EventLog;
use crate::snapshot_store::SnapshotStore;
use crate::state_store::StateStore;

/// Combined storage backend with transaction support.
///
/// For in-memory backends, begin/commit/rollback are no-ops.
/// For PostgreSQL, they map to real DB transactions.
pub trait StorageBackend: Send + Sync {
    fn state_store(&self) -> &dyn StateStore;
    fn state_store_mut(&mut self) -> &mut dyn StateStore;
    fn event_log(&self) -> &dyn EventLog;
    fn event_log_mut(&mut self) -> &mut dyn EventLog;
    fn audit_store(&self) -> &dyn AuditStore;
    fn audit_store_mut(&mut self) -> &mut dyn AuditStore;
    fn snapshot_store(&self) -> &dyn SnapshotStore;
    fn snapshot_store_mut(&mut self) -> &mut dyn SnapshotStore;

    /// Begin a transaction. No-op for in-memory.
    fn begin(&mut self) -> Result<(), KernelError> {
        Ok(())
    }

    /// Commit a transaction. No-op for in-memory.
    fn commit(&mut self) -> Result<(), KernelError> {
        Ok(())
    }

    /// Rollback a transaction. No-op for in-memory.
    fn rollback(&mut self) -> Result<(), KernelError> {
        Ok(())
    }
}

/// In-memory storage backend. All operations are immediate, transactions are no-ops.
pub struct InMemoryBackend {
    pub state_store: crate::state_store::InMemoryStateStore,
    pub event_log: crate::event_log::InMemoryEventLog,
    pub audit_store: crate::audit_log::InMemoryAuditStore,
    pub snapshot_store: crate::snapshot_store::InMemorySnapshotStore,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            state_store: crate::state_store::InMemoryStateStore::new(),
            event_log: crate::event_log::InMemoryEventLog::new(),
            audit_store: crate::audit_log::InMemoryAuditStore::new(),
            snapshot_store: crate::snapshot_store::InMemorySnapshotStore::new(),
        }
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for InMemoryBackend {
    fn state_store(&self) -> &dyn StateStore { &self.state_store }
    fn state_store_mut(&mut self) -> &mut dyn StateStore { &mut self.state_store }
    fn event_log(&self) -> &dyn EventLog { &self.event_log }
    fn event_log_mut(&mut self) -> &mut dyn EventLog { &mut self.event_log }
    fn audit_store(&self) -> &dyn AuditStore { &self.audit_store }
    fn audit_store_mut(&mut self) -> &mut dyn AuditStore { &mut self.audit_store }
    fn snapshot_store(&self) -> &dyn SnapshotStore { &self.snapshot_store }
    fn snapshot_store_mut(&mut self) -> &mut dyn SnapshotStore { &mut self.snapshot_store }
}
