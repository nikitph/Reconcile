//! PostgreSQL storage backends for Reconcile.
//!
//! Provides persistent implementations of all 4 storage traits:
//! - `PostgresStateStore` — resources table
//! - `PostgresEventLog` — events table with BIGSERIAL offset
//! - `PostgresAuditStore` — audit_log table
//! - `PostgresSnapshotStore` — snapshots table
//!
//! Migrations are managed by refinery. On connect, all pending
//! migrations are applied automatically.

mod state_store;
mod event_log;
mod audit_log;
mod snapshot_store;
mod backend;
#[cfg(test)]
mod tests;

pub use state_store::PostgresStateStore;
pub use event_log::PostgresEventLog;
pub use audit_log::PostgresAuditStore;
pub use snapshot_store::PostgresSnapshotStore;
pub use backend::PostgresBackend;

use postgres::{Client, NoTls};

mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("migrations");
}

/// Connect to PostgreSQL and run all pending migrations via refinery.
pub fn connect(database_url: &str) -> Result<Client, Box<dyn std::error::Error>> {
    let mut client = Client::connect(database_url, NoTls)?;
    embedded::migrations::runner().run(&mut client)?;
    Ok(client)
}
