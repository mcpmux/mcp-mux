//! Database integration tests
//!
//! Tests for SQLite repositories, migrations, and transactions.
//!
//! Covers:
//! - Space repository (CRUD, default space)
//! - InstalledServer repository (CRUD, enable/disable)
//! - InboundClient repository (DCR, OAuth tokens, grants)
//! - FeatureSet repository (builtin types, members)
//! - Outbound OAuth repository (server credentials)

mod feature_set;
mod inbound_client;
mod installed_server;
mod migrations;
mod outbound_oauth;
mod repositories;
mod space_base_dir;
