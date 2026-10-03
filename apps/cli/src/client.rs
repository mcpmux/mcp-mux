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

/// Connected control-socket client.
#[cfg(unix)]
pub struct ControlClient {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
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
        let (read_half, write_half) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(read_half),
            writer: write_half,
        })
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
    let data_dir = match data_dir {
        Some(dir) => mcpmux_runtime::resolve_data_dir(Some(dir))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        None => mcpmux_runtime::default_data_dir(),
    };
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
