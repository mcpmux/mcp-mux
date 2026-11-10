//! Control-socket / Named Pipe server for mcpmuxd.
//!
//! On Unix, binds `$XDG_RUNTIME_DIR/mcpmux/<data-dir-id>/control.sock` (`0700` directory, `0600`
//! socket), authenticates the connecting peer by Unix UID, and serves the
//! versioned JSON protocol in `mcpmux-control`.
//!
//! On Windows, binds `\\.\pipe\mcpmux-<data-dir-id>` as a local Named Pipe with
//! default user-restricted security descriptors (`PIPE_REJECT_REMOTE_CLIENTS`).
//! The transport is never exposed over the network; it is a local IPC surface only.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mcpmux_control::{
    codes, read_frame, write_frame, EventEnvelope, Method, RequestEnvelope, ResponseEnvelope,
    PROTOCOL_VERSION,
};
use tokio::io::BufReader;
use tracing::{debug, info, warn};

#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

#[cfg(windows)]
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

use super::handlers::dispatch;
use super::ControlState;

/// A running control-socket/pipe listener. Dropping it removes the socket file on Unix.
pub struct ControlServer {
    socket_path: PathBuf,
    task: tokio::task::JoinHandle<()>,
}

impl ControlServer {
    /// Bind the socket or pipe and start accepting connections.
    ///
    /// Fails when another live daemon already owns the endpoint. A leftover
    /// Unix socket file from an unclean shutdown (connect fails) is replaced.
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

    #[cfg(windows)]
    pub async fn spawn(state: Arc<ControlState>) -> anyhow::Result<Self> {
        let pipe_path = mcpmux_runtime::control_socket_path(&state.runtime.data_dir);
        let pipe_name = pipe_path.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "control pipe path is not valid utf-8: {}",
                pipe_path.display()
            )
        })?;

        let server = match ServerOptions::new()
            .first_pipe_instance(true)
            .create(pipe_name)
        {
            Ok(server) => server,
            Err(err) => {
                // If another daemon is already responding on this pipe, return friendly error.
                if ClientOptions::new().open(pipe_name).is_ok() {
                    anyhow::bail!(
                        "another McpMux daemon is already listening on {}",
                        pipe_path.display()
                    );
                }
                return Err(anyhow::anyhow!(
                    "cannot create control pipe {}: {err}",
                    pipe_path.display()
                ));
            }
        };

        info!(path = %pipe_path.display(), "[control] listening");

        let task = tokio::spawn(accept_loop(server, pipe_name.to_string(), state));
        Ok(Self {
            socket_path: pipe_path,
            task,
        })
    }

    #[cfg(not(any(unix, windows)))]
    pub async fn spawn(_state: Arc<ControlState>) -> anyhow::Result<Self> {
        anyhow::bail!("the mcpmuxd control socket is currently supported only on Unix and Windows")
    }

    /// Path of the bound socket or pipe (useful for tests and status output).
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.task.abort();
        #[cfg(unix)]
        {
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

#[cfg(windows)]
async fn accept_loop(mut server: NamedPipeServer, pipe_name: String, state: Arc<ControlState>) {
    loop {
        match server.connect().await {
            Ok(()) => {
                let connected_client = server;
                // Construct the next server instance before spawning the connection task
                // so subsequent clients never hit NotFound / PIPE_BUSY.
                match ServerOptions::new().create(&pipe_name) {
                    Ok(next_server) => {
                        server = next_server;
                    }
                    Err(e) => {
                        warn!(error = %e, "[control] failed to create next named pipe instance");
                        break;
                    }
                }
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(state, connected_client).await {
                        debug!(error = %e, "[control] connection closed");
                    }
                });
            }
            Err(e) => {
                warn!(error = %e, "[control] accept failed");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                match ServerOptions::new().create(&pipe_name) {
                    Ok(next_server) => server = next_server,
                    Err(err) => {
                        warn!(error = %err, "[control] failed to recreate named pipe server");
                        break;
                    }
                }
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

    let (read_half, write_half) = stream.into_split();
    serve_control_stream(state, BufReader::new(read_half), write_half).await
}

#[cfg(windows)]
async fn handle_connection(
    state: Arc<ControlState>,
    stream: NamedPipeServer,
) -> anyhow::Result<()> {
    let client_pid = get_named_pipe_client_pid(&stream);
    debug!(client_pid = ?client_pid, "[control] accepted named pipe connection");

    let (read_half, write_half) = tokio::io::split(stream);
    serve_control_stream(state, BufReader::new(read_half), write_half).await
}

#[cfg(windows)]
fn get_named_pipe_client_pid(pipe: &NamedPipeServer) -> Option<u32> {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetNamedPipeClientProcessId(
            pipe: *mut std::ffi::c_void,
            client_process_id: *mut u32,
        ) -> i32;
    }
    let mut pid: u32 = 0;
    let handle = pipe.as_raw_handle();
    // SAFETY: handle is a valid named pipe handle and pid is a valid stack pointer.
    let success = unsafe { GetNamedPipeClientProcessId(handle, &mut pid) };
    if success != 0 {
        Some(pid)
    } else {
        None
    }
}

async fn serve_control_stream<R, W>(
    state: Arc<ControlState>,
    mut reader: R,
    mut write_half: W,
) -> anyhow::Result<()>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
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
