//! Native-dialog approval broker for meta-tool writes.
//!
//! Facade module: types live in [`approval_types`], broker logic in
//! [`approval_broker`]. External callers import via `meta_tools::approval::`
//! or the `mod.rs` re-exports — unchanged from before the Phase 8 split.

#[path = "approval_broker.rs"]
mod approval_broker;
#[path = "approval_types.rs"]
mod approval_types;

pub use approval_broker::{
    ApprovalBroker, ApprovalDecision, ApprovalPublisher, ApprovalScope, ResolutionNotifier,
    META_TOOL_APPROVAL_EVENT, META_TOOL_APPROVAL_RESOLVED_EVENT,
};
pub use approval_types::{ApprovalPayload, ApprovalRequest};
