//! Admin REST handlers (Phase 2+).

pub mod error;
pub mod events;
pub mod health;
pub mod oauth;
pub mod read;
pub mod spa;
pub mod write;

pub use health::health;
