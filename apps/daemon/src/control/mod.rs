//! Local control socket: request handling and the Unix listener.

#[cfg(unix)]
pub mod handlers;
#[cfg(not(unix))]
mod handlers_unsupported;
pub mod server;

#[cfg(unix)]
pub use handlers::ControlState;
#[cfg(not(unix))]
pub use handlers_unsupported::ControlState;
pub use server::ControlServer;
