//! Shared backend entry for Tauri commands and admin HTTP handlers.
//!
//! Each submodule mirrors a Tauri command group (`commands/*.rs`). Handlers
//! delegate here so business logic is not duplicated across IPC and REST.

pub mod oauth;
pub mod read;
pub mod space;
pub mod write;
