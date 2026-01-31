//! Event Consumers - Domain event handlers
//!
//! Consumers subscribe to DomainEvents from the EventBus and react based on their
//! specific context:
//!
//! - **MCPNotifier**: Sends MCP list_changed notifications to connected clients
//! - **OAuthEventHandler**: Handles OAuth-related events
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     EventBus (DomainEvent)                      │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!         ┌────────────────────┼────────────────────┐
//!         │                    │                    │
//!         ▼                    ▼                    ▼
//!   ┌───────────┐       ┌───────────┐       ┌─────────────┐
//!   │MCPNotifier│       │Tauri Event│       │ AuditLogger │
//!   │           │       │  Bridge   │       │  (future)   │
//!   └───────────┘       └───────────┘       └─────────────┘
//!         │                    │
//!         ▼                    ▼
//!   list_changed          Tauri emit
//!   to MCP clients        to React UI
//! ```
//!
//! Note: UIEventBridge functionality is now directly in Tauri's gateway.rs
//! via `start_domain_event_bridge()` for tighter integration.

mod mcp_notifier;
mod oauth_handler;

pub use mcp_notifier::MCPNotifier;
pub use oauth_handler::OAuthEventHandler;

