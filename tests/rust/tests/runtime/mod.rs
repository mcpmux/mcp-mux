//! Integration tests for `mcpmux-runtime`.
//!
//! Every test owns its `TempDir` per the headless CLI roadmap's test
//! strategy: no test points at a developer's real data directory or
//! credential store. Port assignments use a monotonically-increasing
//! counter starting at `14518` so multiple test binaries don't collide.

mod bootstrap;
mod event_bridge;
mod lock;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};

use tempfile::TempDir;

/// Pick the next TCP port for a gateway listener in a test.
///
/// Tests run sequentially within a binary; the counter starts at a
/// non-default high port (`14518`) so it does not clash with the
/// production gateway port (`45818`) or with anything else the
/// developer may be running locally. Each test binary gets its own
/// counter because we use a `static AtomicU16` per binary.
pub fn next_test_port() -> u16 {
    static NEXT: AtomicU16 = AtomicU16::new(14_518);
    NEXT.fetch_add(1, Ordering::SeqCst)
}

/// A disposable data directory + a path inside it. Tests use this to
/// bootstrap a `mcpmux_runtime::RuntimeBuilder` against an isolated
/// fixture, then drop the `TempDir` at end of scope (which also
/// releases the runtime's exclusive lock via Drop).
pub struct Fixture {
    pub dir: TempDir,
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().expect("tempdir"),
        }
    }

    pub fn data_dir(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }
}
