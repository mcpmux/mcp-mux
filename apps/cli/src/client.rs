//! Control-socket client for the CLI.

use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
#[cfg(unix)]
use anyhow::{bail, Context};
#[cfg(unix)]
use mcpmux_control::{
    read_frame, write_frame, EventEnvelope, Method, RequestEnvelope, ResponseEnvelope,
    PROTOCOL_VERSION,
};
#[cfg(unix)]
use serde::Serialize;
#[cfg(unix)]
use tokio::io::BufReader;
#[cfg(unix)]
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
#[cfg(unix)]
use tokio::net::UnixStream;

#[cfg(unix)]
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Refuse a socket served by another user than `own_uid`.
#[cfg(unix)]
fn check_peer_uid(path: &Path, peer_uid: u32, own_uid: u32) -> Result<()> {
    if peer_uid != own_uid {
        bail!(
            "{} is served by another user (uid {}); refusing to use it",
            path.display(),
            peer_uid
        );
    }
    Ok(())
}

/// Connected control-socket client.
#[cfg(unix)]
pub struct ControlClient {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    /// Pid of the process serving the socket, when the OS reports it.
    peer_pid: Option<i32>,
}

#[cfg(unix)]
impl ControlClient {
    /// Connect to the daemon socket. The path is derived exactly as the daemon
    /// derives it, so both agree without extra configuration.
    pub async fn connect(data_dir: Option<&Path>, socket: Option<&Path>) -> Result<Self> {
        let path = resolve_socket_path(data_dir, socket)?;
        let stream = UnixStream::connect(&path).await.with_context(|| {
            format!(
                "cannot reach the McpMux daemon on {} — is mcpmuxd running? \
                 (start it with: systemctl --user start mcpmux.service)",
                path.display()
            )
        })?;
        // Only talk to a daemon run by this user: a socket elsewhere (a shared
        // `--socket` path, a planted file) could otherwise collect our
        // requests and steer `daemon restart` at any pid.
        let peer = stream
            .peer_cred()
            .with_context(|| format!("cannot identify the process behind {}", path.display()))?;
        // SAFETY: geteuid has no preconditions and cannot fail.
        let uid = unsafe { libc::geteuid() };
        check_peer_uid(&path, peer.uid(), uid)?;
        let (read_half, write_half) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(read_half),
            writer: write_half,
            peer_pid: peer.pid(),
        })
    }

    /// Pid of the process serving the socket, when the OS reports it.
    pub fn peer_pid(&self) -> Option<i32> {
        self.peer_pid
    }

    /// Send one request and decode the response, mapping daemon errors to
    /// `anyhow` errors tagged with the stable wire code.
    pub async fn call(
        &mut self,
        method: Method,
        params: impl Serialize,
    ) -> Result<serde_json::Value> {
        let request_id = next_request_id();
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            request_id: request_id.clone(),
            method: method.as_str().to_string(),
            params: serde_json::to_value(params)?,
        };
        write_frame(&mut self.writer, &request).await?;

        let response: ResponseEnvelope = read_frame(&mut self.reader)
            .await
            .context("daemon closed the control connection")?;

        if response.request_id != request_id {
            bail!(
                "control protocol desync: expected response {}, got {}",
                request_id,
                response.request_id
            );
        }

        if !response.ok {
            let error = response.error.unwrap_or(mcpmux_control::ErrorBody {
                code: mcpmux_control::codes::INTERNAL.to_string(),
                message: "unknown daemon error".to_string(),
            });
            bail!("{}: {}", error.code, error.message);
        }

        Ok(response.data.unwrap_or(serde_json::Value::Null))
    }

    /// Subscribe to the daemon event stream, invoking `on_event` for each
    /// event until the daemon disconnects or the process is interrupted.
    pub async fn subscribe<F>(&mut self, mut on_event: F) -> Result<()>
    where
        F: FnMut(&EventEnvelope),
    {
        let request_id = next_request_id();
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            request_id: request_id.clone(),
            method: Method::EventsSubscribe.as_str().to_string(),
            params: serde_json::json!({}),
        };
        write_frame(&mut self.writer, &request).await?;

        let ack: ResponseEnvelope = read_frame(&mut self.reader)
            .await
            .context("daemon closed the control connection")?;
        if !ack.ok {
            let message = ack
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "unknown error".to_string());
            bail!("events.subscribe failed: {message}");
        }

        loop {
            let envelope: EventEnvelope = match read_frame(&mut self.reader).await {
                Ok(envelope) => envelope,
                Err(mcpmux_control::FrameError::Closed) => return Ok(()),
                Err(e) => bail!("event stream error: {e}"),
            };
            on_event(&envelope);
        }
    }
}

/// Placeholder client for platforms whose secure local control transport has
/// not been implemented yet.
#[cfg(not(unix))]
pub struct ControlClient;

#[cfg(not(unix))]
impl ControlClient {
    pub async fn connect(_data_dir: Option<&Path>, _socket: Option<&Path>) -> Result<Self> {
        anyhow::bail!(
            "mcpmux-cli currently requires the Unix control socket; use the desktop app on Windows"
        )
    }

    pub async fn call(
        &mut self,
        _method: mcpmux_control::Method,
        _params: impl serde::Serialize,
    ) -> Result<serde_json::Value> {
        anyhow::bail!("mcpmux-cli currently requires the Unix control socket")
    }

    pub async fn subscribe<F>(&mut self, _on_event: F) -> Result<()>
    where
        F: FnMut(&mcpmux_control::EventEnvelope),
    {
        anyhow::bail!("mcpmux-cli currently requires the Unix control socket")
    }
}

/// Resolve the socket path the same way the daemon does.
#[cfg(unix)]
pub fn resolve_socket_path(data_dir: Option<&Path>, socket: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = socket {
        return Ok(path.to_path_buf());
    }
    // Same resolution as the daemon's RuntimeBuilder, so the per-data-dir
    // socket path matches.
    let data_dir =
        mcpmux_runtime::resolve_data_dir(data_dir).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(mcpmux_runtime::control_socket_path(&data_dir))
}

#[cfg(not(unix))]
pub fn resolve_socket_path(_data_dir: Option<&Path>, _socket: Option<&Path>) -> Result<PathBuf> {
    anyhow::bail!("mcpmux-cli currently requires the Unix control socket")
}

#[cfg(unix)]
fn next_request_id() -> String {
    let n = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", std::process::id(), n)
}

#[cfg(all(test, unix))]
mod peer_tests {
    use super::check_peer_uid;
    use std::path::Path;

    #[test]
    fn a_socket_served_by_another_user_is_refused() {
        let path = Path::new("/run/user/1000/mcpmux/control.sock");
        assert!(check_peer_uid(path, 1000, 1000).is_ok());
        let err = check_peer_uid(path, 0, 1000).unwrap_err();
        assert!(err.to_string().contains("another user"), "{err}");
    }
}
