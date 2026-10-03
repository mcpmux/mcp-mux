//! `mcpmux-cli` binary entry point.

use std::process::ExitCode;

use clap::Parser;
use mcpmux_cli::args::Cli;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    reset_sigpipe();
    let cli = Cli::parse();
    match mcpmux_cli::run(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

/// Restore the default `SIGPIPE` disposition so piping into `head`/`grep`
/// terminates quietly instead of panicking on a broken stdout pipe.
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: setting a signal handler to the default disposition.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}
