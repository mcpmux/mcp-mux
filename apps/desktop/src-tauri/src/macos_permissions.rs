//! macOS TCC (Transparency, Consent, and Control) permission registration.
//!
//! McpMux spawns child MCP server processes that may read TCC-restricted
//! resources (Contacts, Calendar, Reminders, AppleEvents). macOS evaluates
//! TCC against the *responsible process* — for child processes spawned via
//! posix_spawn, that's the McpMux app bundle. So McpMux itself must:
//!
//! 1. Declare `NS*UsageDescription` keys in its Info.plist (done in
//!    `apps/desktop/src-tauri/Info.plist`).
//! 2. Call into the corresponding framework once at runtime so macOS
//!    registers the bundle in System Settings → Privacy & Security →
//!    <category> and the user can toggle access on.
//!
//! Without (2), the System Settings panel never lists McpMux at all (Apple
//! only shows apps that have actually requested access), and every child
//! MCP server hits a silent EPERM with no path to fix it.

#[cfg(target_os = "macos")]
use tracing::{debug, info, warn};

/// Triggers the macOS Contacts permission prompt if undetermined.
///
/// Idempotent — safe to call on every app launch. macOS only prompts the
/// first time; subsequent calls return the cached decision instantly.
///
/// Runs the actual `requestAccess` call on a background thread because the
/// completion handler fires on an arbitrary queue and we don't want to
/// block the Tauri setup hook.
#[cfg(target_os = "macos")]
pub fn ensure_contacts_registered() {
    use objc2_contacts::{CNAuthorizationStatus, CNContactStore, CNEntityType};

    // SAFETY: `authorizationStatusForEntityType` is a pure read of the
    // system TCC database — no side effects, no main-thread requirement.
    let status =
        unsafe { CNContactStore::authorizationStatusForEntityType(CNEntityType::Contacts) };

    match status {
        CNAuthorizationStatus::Authorized => {
            debug!("[Permissions] Contacts: authorized");
        }
        CNAuthorizationStatus::Denied => {
            warn!(
                "[Permissions] Contacts: denied. Child MCP servers reading AddressBook \
                 will fail with EPERM until granted in System Settings → Privacy & Security \
                 → Contacts → McpMux."
            );
        }
        CNAuthorizationStatus::Restricted => {
            warn!("[Permissions] Contacts: restricted by system policy.");
        }
        CNAuthorizationStatus::NotDetermined => {
            info!(
                "[Permissions] Contacts: not determined — requesting access \
                 to register McpMux in the system Privacy panel."
            );
            request_contacts_access();
        }
        // CNAuthorizationStatus is non-exhaustive — newer macOS versions may
        // add cases (e.g. Limited). Treat unknown as a soft warning rather
        // than re-prompting, since requestAccess is a no-op past first call.
        other => {
            debug!(
                ?other,
                "[Permissions] Contacts: unknown status, skipping prompt"
            );
        }
    }
}

/// Calls `CNContactStore.requestAccess(for:.contacts, completionHandler:)`.
///
/// macOS shows the system prompt on first call only; the completion handler
/// fires on an arbitrary queue and the result is cached in TCC.db. We log
/// the outcome but don't block on it — by the time the user clicks Allow/Deny
/// the app is already running.
#[cfg(target_os = "macos")]
fn request_contacts_access() {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_contacts::{CNContactStore, CNEntityType};
    use objc2_foundation::NSError;

    let store: Retained<CNContactStore> = unsafe { CNContactStore::new() };

    // Block fires on an arbitrary GCD queue once the user dismisses the
    // prompt (or immediately, if a decision is cached).
    let handler = RcBlock::new(|granted: Bool, error: *mut NSError| {
        if granted.as_bool() {
            info!("[Permissions] Contacts: user granted access");
        } else if !error.is_null() {
            // SAFETY: CN guarantees error is a valid NSError when non-null.
            let err = unsafe { &*error };
            warn!(
                "[Permissions] Contacts request failed: {}",
                err.localizedDescription()
            );
        } else {
            warn!("[Permissions] Contacts: user denied access");
        }
    });

    // SAFETY: `requestAccessForEntityType_completionHandler` is the
    // documented entry point. The block stays alive for the duration of
    // the async request because RcBlock retains it.
    unsafe {
        store.requestAccessForEntityType_completionHandler(CNEntityType::Contacts, &handler);
    }
}

/// No-op on non-macOS platforms.
#[cfg(not(target_os = "macos"))]
pub fn ensure_contacts_registered() {}
