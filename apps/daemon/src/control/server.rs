//! Unix control-socket server.
//!
//! Binds `$XDG_RUNTIME_DIR/mcpmux/control.sock` (`0700` directory, `0600`
//! socket), authenticates the connecting peer by Unix UID, and serves the
//! versioned JSON protocol in `mcpmux-control`. The socket is never exposed
//! over the network; it is a local IPC surface only.

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(unix)]
use mcpmux_control::{
    codes, read_frame, write_frame, EventEnvelope, Method, RequestEnvelope, ResponseEnvelope,
    PROTOCOL_VERSION,
};
#[cfg(unix)]
use tokio::io::BufReader;
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
#[cfg(unix)]
use tracing::{debug, info, warn};

#[cfg(unix)]
use super::handlers::dispatch;
use super::ControlState;

/// A running control-socket listener. Dropping it removes the socket file.
pub struct ControlServer {
    socket_path: PathBuf,
    #[cfg(unix)]
    task: tokio::task::JoinHandle<()>,
}

impl ControlServer {
    /// Bind the socket and start accepting connections.
    ///
    /// Fails when another live daemon already owns the socket. A leftover
    /// socket file from an unclean shutdown (connect fails) is replaced.
    #[cfg(unix)]
    pub async fn spawn(state: Arc<ControlState>) -> anyhow::Result<Self> {
        let dir = mcpmux_runtime::control_dir(&state.runtime.data_dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| anyhow::anyhow!("cannot create {}: {e}", dir.display()))?;
        restrict_dir_permissions(&dir)?;

        let socket_path = dir.join("control.sock");
        if socket_path.exists() {
            match UnixStream::connect(&socket_path).await {
                Ok(_) => anyhow::bail!(
                    "another McpMux daemon is already listening on {}",
                    socket_path.display()
                ),
                Err(_) => {
                    // Stale socket left by a crashed process.
                    let _ = std::fs::remove_file(&socket_path);
                }
            }
        }

        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| anyhow::anyhow!("cannot bind {}: {e}", socket_path.display()))?;
        restrict_socket_permissions(&socket_path)?;

        info!(path = %socket_path.display(), "[control] listening");

        let task = tokio::spawn(accept_loop(listener, state));
        Ok(Self { socket_path, task })
    }

    /// Path of the bound socket (useful for tests and status output).
    #[cfg(unix)]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

#[cfg(not(unix))]
impl ControlServer {
    pub async fn spawn(_state: Arc<ControlState>) -> anyhow::Result<Self> {
        anyhow::bail!("the mcpmuxd control socket is currently supported only on Unix")
    }

    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            self.task.abort();
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

#[cfg(unix)]
async fn accept_loop(listener: UnixListener, state: Arc<ControlState>) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(state, stream).await {
                        debug!(error = %e, "[control] connection closed");
                    }
                });
            }
            Err(e) => {
                warn!(error = %e, "[control] accept failed");
            }
        }
    }
}

#[cfg(unix)]
async fn handle_connection(state: Arc<ControlState>, stream: UnixStream) -> anyhow::Result<()> {
    // Authenticate by peer UID: only the owning user may control the daemon.
    let peer = stream
        .peer_cred()
        .map_err(|e| anyhow::anyhow!("cannot read peer credentials: {e}"))?;
    let expected = current_uid();
    if peer.uid() != expected {
        warn!(
            peer_uid = peer.uid(),
            expected_uid = expected,
            "[control] rejecting connection from a different user"
        );
        return Ok(());
    }

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    loop {
        let request: RequestEnvelope = match read_frame(&mut reader).await {
            Ok(request) => request,
            Err(mcpmux_control::FrameError::Closed) => return Ok(()),
            Err(e) => {
                let response = ResponseEnvelope::error("", codes::BAD_FRAME, e.to_string());
                let _ = write_frame(&mut write_half, &response).await;
                return Ok(());
            }
        };

        if request.version != PROTOCOL_VERSION {
            let response = ResponseEnvelope::error(
                request.request_id,
                codes::INCOMPATIBLE_VERSION,
                format!(
                    "protocol version {} is not supported (daemon speaks {})",
                    request.version, PROTOCOL_VERSION
                ),
            );
            let _ = write_frame(&mut write_half, &response).await;
            return Ok(());
        }

        if Method::parse(&request.method) == Some(Method::EventsSubscribe) {
            let ack = ResponseEnvelope::ok(
                &request.request_id,
                &serde_json::json!({"subscribed": true}),
            );
            write_frame(&mut write_half, &ack).await?;
            stream_events(&state, &mut write_half).await;
            return Ok(());
        }

        let response = match dispatch(&state, &request).await {
            Ok(data) => ResponseEnvelope::ok(&request.request_id, &data),
            Err(err) => ResponseEnvelope::error(&request.request_id, err.code, err.message),
        };

        if write_frame(&mut write_half, &response).await.is_err() {
            return Ok(());
        }
    }
}

#[cfg(unix)]
async fn stream_events<W>(state: &ControlState, writer: &mut W)
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut receiver = state.runtime.subscribe_events();
    while let Some(event) = receiver.recv().await {
        let envelope = EventEnvelope::new(event);
        if write_frame(writer, &envelope).await.is_err() {
            return;
        }
    }
}

#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: geteuid is always safe to call.
    unsafe { libc::geteuid() as u32 }
}

#[cfg(unix)]
fn restrict_dir_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| anyhow::anyhow!("cannot set permissions on {}: {e}", path.display()))
}

#[cfg(unix)]
fn restrict_socket_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| anyhow::anyhow!("cannot set permissions on {}: {e}", path.display()))
}
