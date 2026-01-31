//! MCP Server Registry
//!
//! This module defines the schema for the MCP server registry.
//! The registry can be loaded from a local JSON file or fetched from a remote API.

mod schema;
mod types;
mod validation;

pub use schema::*;
pub use types::*;
pub use validation::*;
