//! Centralized Logging Infrastructure
//!
//! Provides structured logging with:
//! - Trace IDs for request correlation
//! - Colored console output
//! - File logging with rotation
//! - Reduced verbosity through consolidation

mod trace_context;

pub use trace_context::{TraceContext, RequestSpan};
