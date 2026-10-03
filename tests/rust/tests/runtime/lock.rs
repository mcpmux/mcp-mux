//! Concurrent-acquire test for the data-dir exclusive lock.
//!
//! Two `RuntimeBuilder::build()` calls in the same process cannot both
//! hold the lock; the second one must surface a `LockHeld` error that
//! names the owner PID. Mirrors the cross-process safety guarantee the
//! runtime needs to provide so the desktop and daemon never silently
//! share a data directory.

use mcpmux_runtime::RuntimeBuilder;

use super::Fixture;

#[tokio::test]
async fn second_build_against_held_data_dir_fails_with_owner_info() {
    let fx = Fixture::new();

    let runtime = RuntimeBuilder::new()
        .with_data_dir(fx.data_dir())
        .build()
        .await
        .expect("first runtime");

    let err = match RuntimeBuilder::new()
        .with_data_dir(fx.data_dir())
        .build()
        .await
    {
        Ok(_) => panic!("second runtime should refuse while first holds the lock"),
        Err(e) => e,
    };

    assert!(err.is_lock_held(), "expected LockHeld variant, got {err}");

    // The operator-facing error must identify the owner — no silent
    // repairs, no generic "permission denied" message.
    let display = err.to_string();
    assert!(
        display.contains("pid"),
        "operator error should mention pid, got: {display}"
    );
    assert!(
        display.contains("v0.") || display.contains("version"),
        "operator error should mention the owner version so an operator can tell \
         whether the lock is from a stale process or the running daemon, got: {display}",
    );

    // Dropping the first runtime releases the lock.
    drop(runtime);
}
