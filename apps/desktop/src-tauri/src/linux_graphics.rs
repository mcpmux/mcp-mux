//! Startup fallback for Linux GPUs that WebKitGTK can't drive (issue #237).
//!
//! WebKitGTK's web process always creates an EGL display — even with hardware
//! acceleration turned off (verified on 2.52.6) — and a failure there does not
//! fall through: when the GPU driver can't initialize (e.g. a GPU newer than
//! the installed Mesa) WebKit logs `Could not create surfaceless EGL display:
//! EGL_BAD_ALLOC. Aborting...` and crashes, so the window never renders. No
//! `WEBKIT_*` variable avoids that, so the fix is to make EGL work instead.
//!
//! Before WebKit starts, [`configure`] runs WebKit's EGL setup in a forked
//! child, so a crashing driver only takes the child down:
//!   1. On the GPU. If that works, nothing changes.
//!   2. Otherwise with `LIBGL_ALWAYS_SOFTWARE=1` (Mesa's CPU renderer). If that
//!      works, we set it — plus `WEBKIT_DISABLE_DMABUF_RENDERER=1`, so WebKit
//!      skips the GBM/DMA-BUF path and uses the surfaceless display we tested.
//!   3. If neither works there's nothing to fall back to; we log what to
//!      install rather than claim a fix.
//!
//! The verdict is cached against a fingerprint of the GPUs, drivers, EGL
//! vendor configs and app version, so the probe only reruns when one of those
//! changes. Probing on every launch would cost ~1.6 s on hybrid laptops: libglvnd
//! offers the display to the NVIDIA driver first, which wakes a suspended dGPU.
//!
//! Overrides: `MCPMUX_SAFE_GRAPHICS=1` forces the software fallback, `=0` skips
//! all of this. If the user set any of the variables we'd set, we leave them.

use std::ffi::{c_char, c_void, CStr};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use tracing::{error, info, warn};

const SOFTWARE_VAR: &str = "LIBGL_ALWAYS_SOFTWARE";
const DMABUF_VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
/// Set to `1` together for the software fallback; if the user set any of them
/// they're managing WebKit's GPU use themselves.
const FALLBACK_VARS: [&str; 2] = [SOFTWARE_VAR, DMABUF_VAR];
const OVERRIDE_VAR: &str = "MCPMUX_SAFE_GRAPHICS";

/// Variables that change which EGL driver loads; part of the cache key so a
/// verdict from one configuration is never reused for another.
const EGL_ENV_VARS: [&str; 7] = [
    SOFTWARE_VAR,
    "__EGL_VENDOR_LIBRARY_FILENAMES",
    "__EGL_VENDOR_LIBRARY_DIRS",
    "MESA_LOADER_DRIVER_OVERRIDE",
    "GALLIUM_DRIVER",
    "DRI_PRIME",
    "EGL_PLATFORM",
];

/// Long enough for a cold driver load; a driver that hangs here would hang
/// WebKit too, so a timeout counts as a failure.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// What startup did about WebKit's GPU use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// WebKit's defaults are untouched.
    Unchanged { reason: String },
    /// [`FALLBACK_VARS`] were set to `1`.
    SoftwareRendering { reason: String },
    /// EGL fails even in software; nothing set (WebKit will likely abort).
    Unavailable { reason: String },
}

/// A probe verdict worth caching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Hardware,
    Software,
}

impl Verdict {
    fn as_str(self) -> &'static str {
        match self {
            Verdict::Hardware => "hardware",
            Verdict::Software => "software",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "hardware" => Some(Verdict::Hardware),
            "software" => Some(Verdict::Software),
            _ => None,
        }
    }
}

/// Result of running an EGL probe in a child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Ok,
    /// The probe ran and reported why EGL is unusable.
    Failed(String),
    /// The child died from this signal — what WebKit would have done.
    Crashed(i32),
    TimedOut,
    /// Couldn't run the probe at all (e.g. `fork` failed); no verdict.
    Skipped(String),
}

impl std::fmt::Display for ProbeOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeOutcome::Ok => write!(f, "EGL display initialized"),
            ProbeOutcome::Failed(why) => write!(f, "{why}"),
            ProbeOutcome::Crashed(signal) => write!(f, "probe crashed with signal {signal}"),
            ProbeOutcome::TimedOut => write!(f, "probe timed out after {PROBE_TIMEOUT:?}"),
            ProbeOutcome::Skipped(why) => write!(f, "probe skipped: {why}"),
        }
    }
}

/// Check that WebKit can get an EGL display and, if only Mesa's software
/// renderer can provide one, switch to it. `cache_file` holds the last verdict.
///
/// Must run before any thread is spawned (tracing's file writer, Tauri): it
/// forks, and it may set environment variables WebKit reads at startup.
pub fn configure(cache_file: &Path) -> Decision {
    let started = Instant::now();
    let user_set = FALLBACK_VARS
        .into_iter()
        .find(|var| std::env::var_os(var).is_some());
    let fingerprint = fingerprint(env!("CARGO_PKG_VERSION"), |var| std::env::var(var).ok());
    let (decision, verdict) = decide(
        user_set,
        std::env::var(OVERRIDE_VAR).ok().as_deref(),
        read_cache(cache_file, &fingerprint),
        || run_isolated(probe_egl, PROBE_TIMEOUT),
        || run_isolated(probe_egl_software, PROBE_TIMEOUT),
    );
    if let Some(verdict) = verdict {
        write_cache(cache_file, &fingerprint, verdict);
    }
    if let Decision::SoftwareRendering { .. } = decision {
        for var in FALLBACK_VARS {
            std::env::set_var(var, "1");
        }
    }
    let ms = started.elapsed().as_millis();
    match decision {
        Decision::Unchanged { reason } => Decision::Unchanged {
            reason: format!("{reason} ({ms} ms)"),
        },
        Decision::SoftwareRendering { reason } => Decision::SoftwareRendering {
            reason: format!("{reason} ({ms} ms)"),
        },
        unavailable => unavailable,
    }
}

/// Log what [`configure`] decided — call once tracing is up.
pub fn log(decision: &Decision) {
    match decision {
        Decision::Unchanged { reason } => info!("[Graphics] GPU rendering: {reason}"),
        Decision::SoftwareRendering { reason } => warn!(
            "[Graphics] Software rendering ({}=1): {reason}. Set {OVERRIDE_VAR}=0 to skip this check.",
            FALLBACK_VARS.join("=1, ")
        ),
        Decision::Unavailable { reason } => error!(
            "[Graphics] No working EGL driver, even in software: {reason}. The window will \
             likely fail to render — install Mesa's EGL/GL drivers (e.g. `mesa` on Arch, \
             `libegl-mesa0` on Debian/Ubuntu)."
        ),
    }
}

/// Pure decision logic, separated from the environment, cache and probes for
/// tests. `user_set` names a [`FALLBACK_VARS`] entry the user already set.
/// Probes only run when nothing earlier decided; `probe_software` only after
/// the hardware probe failed. Returns the verdict to cache, if the probes ran.
fn decide(
    user_set: Option<&str>,
    override_value: Option<&str>,
    cached: Option<Verdict>,
    probe_hardware: impl FnOnce() -> ProbeOutcome,
    probe_software: impl FnOnce() -> ProbeOutcome,
) -> (Decision, Option<Verdict>) {
    if let Some(var) = user_set {
        let reason = format!("{var} set by the user");
        return (Decision::Unchanged { reason }, None);
    }
    match override_value
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("1" | "true" | "yes" | "on") => {
            let reason = format!("forced by {OVERRIDE_VAR}");
            return (Decision::SoftwareRendering { reason }, None);
        }
        Some("0" | "false" | "no" | "off") => {
            let reason = format!("check disabled by {OVERRIDE_VAR}=0");
            return (Decision::Unchanged { reason }, None);
        }
        _ => {}
    }
    match cached {
        Some(Verdict::Hardware) => {
            let reason = "GPU EGL worked last time (cached)".to_string();
            return (Decision::Unchanged { reason }, None);
        }
        Some(Verdict::Software) => {
            let reason = "GPU EGL failed last time (cached)".to_string();
            return (Decision::SoftwareRendering { reason }, None);
        }
        None => {}
    }

    let hardware = probe_hardware();
    match hardware {
        ProbeOutcome::Ok => {
            let reason = "EGL probe passed".to_string();
            return (Decision::Unchanged { reason }, Some(Verdict::Hardware));
        }
        // No verdict: don't change rendering (or cache anything) on a guess.
        ProbeOutcome::Skipped(why) => {
            let reason = format!("EGL probe skipped: {why}");
            return (Decision::Unchanged { reason }, None);
        }
        _ => {}
    }
    match probe_software() {
        ProbeOutcome::Ok => {
            let reason = format!("GPU EGL failed ({hardware}); Mesa's software renderer works");
            (
                Decision::SoftwareRendering { reason },
                Some(Verdict::Software),
            )
        }
        // Not cached: re-check next launch in case drivers get installed.
        software => {
            let reason = format!("GPU: {hardware}; software: {software}");
            (Decision::Unavailable { reason }, None)
        }
    }
}

/// Everything that decides which EGL driver loads, as a stable string. The
/// cached verdict is reused only while this is unchanged. Reads only cached
/// sysfs attributes, so it doesn't wake a suspended GPU.
fn fingerprint(app_version: &str, env: impl Fn(&str) -> Option<String>) -> String {
    let mut lines = vec![format!("app={app_version}")];
    if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        lines.push(format!("kernel={}", release.trim()));
    }
    if let Ok(version) = std::fs::read_to_string("/sys/module/nvidia/version") {
        lines.push(format!("nvidia={}", version.trim()));
    }
    for node in sorted_dir("/sys/class/drm").into_iter().filter(|p| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("renderD"))
    }) {
        let read = |attr: &str| {
            std::fs::read_to_string(node.join("device").join(attr))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        let driver = std::fs::read_link(node.join("device/driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        lines.push(format!(
            "gpu={} {} {} {driver}",
            node.display(),
            read("vendor"),
            read("device")
        ));
    }
    let vendor_dirs: Vec<PathBuf> = env("__EGL_VENDOR_LIBRARY_DIRS")
        .map(|dirs| dirs.split(':').map(PathBuf::from).collect())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/etc/glvnd/egl_vendor.d"),
                PathBuf::from("/usr/share/glvnd/egl_vendor.d"),
            ]
        });
    for json in vendor_dirs.iter().flat_map(sorted_dir) {
        let library = std::fs::read_to_string(&json)
            .ok()
            .and_then(|text| library_path(&text))
            .and_then(|lib| locate_library(&lib));
        lines.push(format!(
            "vendor={} {} lib={}",
            json.display(),
            file_stamp(&json),
            library.as_deref().map(file_stamp).unwrap_or_default()
        ));
    }
    for var in EGL_ENV_VARS {
        if let Some(value) = env(var) {
            lines.push(format!("env {var}={value}"));
        }
    }
    lines.join("\n")
}

fn sorted_dir(dir: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|it| it.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    entries.sort();
    entries
}

/// `ICD.library_path` from a glvnd vendor JSON, without a JSON dependency on
/// this pre-runtime path.
fn library_path(json: &str) -> Option<String> {
    const KEY: &str = "\"library_path\"";
    let after = &json[json.find(KEY)? + KEY.len()..];
    let start = after.find('"')? + 1;
    let len = after[start..].find('"')?;
    Some(after[start..start + len].to_string())
}

fn locate_library(name: &str) -> Option<PathBuf> {
    if name.starts_with('/') {
        return Some(PathBuf::from(name));
    }
    [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/lib64",
        "/usr/lib",
        "/lib/x86_64-linux-gnu",
        "/lib64",
        "/lib",
    ]
    .iter()
    .map(|dir| Path::new(dir).join(name))
    .find(|path| path.exists())
}

/// `size:mtime` — changes whenever a package update replaces the file.
fn file_stamp(path: &Path) -> String {
    std::fs::metadata(path)
        .map(|m| {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("{}:{mtime}", m.len())
        })
        .unwrap_or_else(|_| "missing".into())
}

/// Cache format: the verdict on the first line, the fingerprint after it.
fn read_cache(path: &Path, fingerprint: &str) -> Option<Verdict> {
    let text = std::fs::read_to_string(path).ok()?;
    let (verdict, cached_fingerprint) = text.split_once('\n')?;
    if cached_fingerprint != fingerprint {
        return None;
    }
    Verdict::parse(verdict)
}

/// Best effort: without a cache the probe simply runs again next launch.
fn write_cache(path: &Path, fingerprint: &str, verdict: Verdict) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, format!("{}\n{fingerprint}", verdict.as_str()));
}

/// Run `probe` in a forked child and classify how it ended. The probe's return
/// value becomes the child's exit code (`0` = success, see [`describe_failure`]).
fn run_isolated(probe: fn() -> u8, timeout: Duration) -> ProbeOutcome {
    // SAFETY: `configure` runs before any other thread exists, so the child
    // gets a consistent copy of the process. The child only runs `probe` and
    // `_exit`s — it never returns into the caller or runs destructors/atexit.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return ProbeOutcome::Skipped(format!("fork: {}", std::io::Error::last_os_error()));
    }
    if pid == 0 {
        silence_stdio();
        let code = probe();
        // SAFETY: terminates the child immediately; see above.
        unsafe { libc::_exit(i32::from(code)) }
    }

    let deadline = Instant::now() + timeout;
    loop {
        let mut status = 0;
        // SAFETY: plain FFI on our own child pid with a valid out-pointer.
        let ret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if ret == pid {
            return classify(status);
        }
        if ret < 0 {
            return ProbeOutcome::Skipped(format!("waitpid: {}", std::io::Error::last_os_error()));
        }
        if Instant::now() >= deadline {
            // SAFETY: kill and reap our own child so it doesn't linger.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
                libc::waitpid(pid, &mut status, 0);
            }
            return ProbeOutcome::TimedOut;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn classify(status: i32) -> ProbeOutcome {
    if libc::WIFSIGNALED(status) {
        return ProbeOutcome::Crashed(libc::WTERMSIG(status));
    }
    match libc::WEXITSTATUS(status) {
        0 => ProbeOutcome::Ok,
        code => ProbeOutcome::Failed(describe_failure(code as u8)),
    }
}

/// Keep driver chatter from the probe out of the user's terminal; we log the
/// outcome ourselves.
fn silence_stdio() {
    // SAFETY: opens /dev/null and duplicates it over stdout/stderr in the child.
    unsafe {
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
        if null >= 0 {
            libc::dup2(null, libc::STDOUT_FILENO);
            libc::dup2(null, libc::STDERR_FILENO);
            libc::close(null);
        }
    }
}

// Probe exit codes. eglInitialize failures carry the EGL error so the log can
// say e.g. EGL_BAD_ALLOC, as in the original report.
const NO_LIBRARY: u8 = 2;
const NO_ENTRY_POINT: u8 = 3;
const NO_DISPLAY: u8 = 4;
const INIT_FAILED_BASE: u8 = 16;

const EGL_SUCCESS: i32 = 0x3000;
const EGL_CONTEXT_LOST: i32 = 0x300E;
const EGL_ERROR_NAMES: [&str; 15] = [
    "EGL_SUCCESS",
    "EGL_NOT_INITIALIZED",
    "EGL_BAD_ACCESS",
    "EGL_BAD_ALLOC",
    "EGL_BAD_ATTRIBUTE",
    "EGL_BAD_CONFIG",
    "EGL_BAD_CONTEXT",
    "EGL_BAD_CURRENT_SURFACE",
    "EGL_BAD_DISPLAY",
    "EGL_BAD_MATCH",
    "EGL_BAD_NATIVE_PIXMAP",
    "EGL_BAD_NATIVE_WINDOW",
    "EGL_BAD_PARAMETER",
    "EGL_BAD_SURFACE",
    "EGL_CONTEXT_LOST",
];

fn describe_failure(code: u8) -> String {
    match code {
        NO_LIBRARY => "libEGL.so.1 could not be loaded".into(),
        NO_ENTRY_POINT => "libEGL is missing a required entry point".into(),
        NO_DISPLAY => "no EGL display available".into(),
        c if c >= INIT_FAILED_BASE => {
            let name = EGL_ERROR_NAMES
                .get(usize::from(c - INIT_FAILED_BASE))
                .copied()
                .unwrap_or("unknown EGL error");
            format!("eglInitialize failed: {name}")
        }
        c => format!("probe exited with code {c}"),
    }
}

// EGL constants (from the Khronos headers).
const EGL_EXTENSIONS: i32 = 0x3055;
const EGL_PLATFORM_SURFACELESS_MESA: u32 = 0x31DD;

type EglDisplay = *mut c_void;
type EglGetProcAddress = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type EglQueryString = unsafe extern "C" fn(EglDisplay, i32) -> *const c_char;
type EglGetPlatformDisplayExt = unsafe extern "C" fn(u32, *mut c_void, *const i32) -> EglDisplay;
type EglGetPlatformDisplay = unsafe extern "C" fn(u32, *mut c_void, *const isize) -> EglDisplay;
type EglGetDisplay = unsafe extern "C" fn(*mut c_void) -> EglDisplay;
type EglInitialize = unsafe extern "C" fn(EglDisplay, *mut i32, *mut i32) -> u32;
type EglTerminate = unsafe extern "C" fn(EglDisplay) -> u32;
type EglGetError = unsafe extern "C" fn() -> i32;

/// The software stage: the same probe with Mesa forced onto its CPU renderer.
/// Runs in the forked child, so setting the variable doesn't touch the app.
fn probe_egl_software() -> u8 {
    std::env::set_var(SOFTWARE_VAR, "1");
    probe_egl()
}

/// Mirrors WebKit's web-process display selection — `WebProcess::
/// initializePlatformDisplayIfNeeded()` in `WebProcess/glib/WebProcessGLib.cpp`
/// (checked against WebKitGTK 2.48–2.52): Mesa's surfaceless platform when the
/// client extension is offered, failing hard if it can't be created; else the
/// default display. It skips WebKit's first choice, GBM on the DMA-BUF path —
/// the software fallback disables that path anyway. Re-check this order when
/// WebKitGTK changes it. Runs in the forked child; returns the exit code.
fn probe_egl() -> u8 {
    // SAFETY: dlopen/dlsym on libEGL, and calls through pointers cast to the
    // EGL 1.5 signatures. Only ever runs inside the forked probe child.
    unsafe {
        let lib = libc::dlopen(c"libEGL.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if lib.is_null() {
            return NO_LIBRARY;
        }
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let ptr = libc::dlsym(lib, concat!($name, "\0").as_ptr().cast());
                if ptr.is_null() {
                    return NO_ENTRY_POINT;
                }
                std::mem::transmute::<*mut c_void, $ty>(ptr)
            }};
        }
        let get_proc_address = sym!("eglGetProcAddress", EglGetProcAddress);
        let query_string = sym!("eglQueryString", EglQueryString);
        let get_display = sym!("eglGetDisplay", EglGetDisplay);
        let initialize = sym!("eglInitialize", EglInitialize);
        let terminate = sym!("eglTerminate", EglTerminate);
        let get_error = sym!("eglGetError", EglGetError);

        // Client extensions; null when the implementation has none.
        let extensions = query_string(std::ptr::null_mut(), EGL_EXTENSIONS);
        let has = |name: &str| {
            !extensions.is_null()
                && CStr::from_ptr(extensions)
                    .to_string_lossy()
                    .split_whitespace()
                    .any(|e| e == name)
        };

        let display = if has("EGL_MESA_platform_surfaceless") {
            if has("EGL_EXT_platform_base") {
                let ptr = get_proc_address(c"eglGetPlatformDisplayEXT".as_ptr());
                if ptr.is_null() {
                    return NO_ENTRY_POINT;
                }
                let get_platform_display =
                    std::mem::transmute::<*mut c_void, EglGetPlatformDisplayExt>(ptr);
                get_platform_display(
                    EGL_PLATFORM_SURFACELESS_MESA,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            } else if has("EGL_KHR_platform_base") {
                let get_platform_display = sym!("eglGetPlatformDisplay", EglGetPlatformDisplay);
                get_platform_display(
                    EGL_PLATFORM_SURFACELESS_MESA,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            } else {
                std::ptr::null_mut()
            }
        } else {
            get_display(std::ptr::null_mut())
        };
        if display.is_null() {
            return NO_DISPLAY;
        }

        if initialize(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
            let err = get_error();
            let index = if (EGL_SUCCESS..=EGL_CONTEXT_LOST).contains(&err) {
                (err - EGL_SUCCESS) as u8
            } else {
                EGL_ERROR_NAMES.len() as u8
            };
            return INIT_FAILED_BASE + index;
        }
        terminate(display);
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never_probed() -> ProbeOutcome {
        panic!("probe must not run here")
    }

    fn failed() -> ProbeOutcome {
        ProbeOutcome::Failed("eglInitialize failed: EGL_BAD_ALLOC".into())
    }

    #[test]
    fn user_set_vars_are_left_alone() {
        // Even a forced fallback or a cached verdict defers to the user.
        let (d, cache) = decide(
            Some(DMABUF_VAR),
            Some("1"),
            Some(Verdict::Software),
            never_probed,
            never_probed,
        );
        assert_eq!(
            d,
            Decision::Unchanged {
                reason: format!("{DMABUF_VAR} set by the user")
            }
        );
        assert_eq!(cache, None);
    }

    #[test]
    fn override_forces_or_disables_without_probing() {
        for on in ["1", "true", " YES ", "on"] {
            let (d, cache) = decide(None, Some(on), None, never_probed, never_probed);
            assert!(
                matches!(d, Decision::SoftwareRendering { .. }),
                "{on}: {d:?}"
            );
            assert_eq!(cache, None);
        }
        for off in ["0", "false", "No", "off"] {
            let (d, _) = decide(
                None,
                Some(off),
                Some(Verdict::Software),
                never_probed,
                never_probed,
            );
            assert!(matches!(d, Decision::Unchanged { .. }), "{off}: {d:?}");
        }
    }

    #[test]
    fn cached_verdict_skips_both_probes() {
        let (d, cache) = decide(
            None,
            None,
            Some(Verdict::Hardware),
            never_probed,
            never_probed,
        );
        assert!(matches!(d, Decision::Unchanged { .. }), "{d:?}");
        assert_eq!(cache, None);
        let (d, _) = decide(
            None,
            None,
            Some(Verdict::Software),
            never_probed,
            never_probed,
        );
        assert!(matches!(d, Decision::SoftwareRendering { .. }), "{d:?}");
    }

    #[test]
    fn working_gpu_changes_nothing_and_is_cached() {
        let (d, cache) = decide(None, Some("maybe"), None, || ProbeOutcome::Ok, never_probed);
        assert!(matches!(d, Decision::Unchanged { .. }), "{d:?}");
        assert_eq!(cache, Some(Verdict::Hardware));
    }

    #[test]
    fn failing_gpu_falls_back_when_software_works() {
        // Every way the GPU stage can fail leads to the software check.
        for hardware in [
            failed(),
            ProbeOutcome::Crashed(libc::SIGABRT),
            ProbeOutcome::TimedOut,
        ] {
            let (d, cache) = decide(None, None, None, || hardware.clone(), || ProbeOutcome::Ok);
            assert!(matches!(d, Decision::SoftwareRendering { .. }), "{d:?}");
            assert_eq!(cache, Some(Verdict::Software));
        }
    }

    #[test]
    fn nothing_working_claims_no_fix_and_is_not_cached() {
        let (d, cache) = decide(None, None, None, failed, || ProbeOutcome::TimedOut);
        assert_eq!(
            d,
            Decision::Unavailable {
                reason: format!(
                    "GPU: eglInitialize failed: EGL_BAD_ALLOC; software: probe timed out after {PROBE_TIMEOUT:?}"
                )
            }
        );
        assert_eq!(cache, None);
    }

    #[test]
    fn unprobeable_system_is_left_alone() {
        let (d, cache) = decide(
            None,
            None,
            None,
            || ProbeOutcome::Skipped("fork".into()),
            never_probed,
        );
        assert!(matches!(d, Decision::Unchanged { .. }), "{d:?}");
        assert_eq!(cache, None);
    }

    #[test]
    fn fingerprint_tracks_app_version_and_egl_env() {
        let none = |_: &str| None;
        let base = fingerprint("1.0.0", none);
        assert_eq!(base, fingerprint("1.0.0", none), "must be stable");
        assert_ne!(base, fingerprint("1.0.1", none));
        let with_override = fingerprint("1.0.0", |var| {
            (var == "MESA_LOADER_DRIVER_OVERRIDE").then(|| "radeonsi".to_string())
        });
        assert_ne!(base, with_override);
        assert!(with_override.contains("env MESA_LOADER_DRIVER_OVERRIDE=radeonsi"));
    }

    #[test]
    fn glvnd_library_path_is_parsed() {
        let json = r#"{ "file_format_version" : "1.0.0", "ICD" : { "library_path" : "libEGL_mesa.so.0" } }"#;
        assert_eq!(library_path(json).as_deref(), Some("libEGL_mesa.so.0"));
        assert_eq!(library_path("{}"), None);
    }

    #[test]
    fn cache_round_trips_and_ignores_stale_or_corrupt_entries() {
        let dir = std::env::temp_dir().join(format!("mcpmux-graphics-test-{}", std::process::id()));
        let path = dir.join("nested").join("graphics-probe");
        assert_eq!(read_cache(&path, "fp"), None);

        write_cache(&path, "fp\nline2", Verdict::Software);
        assert_eq!(read_cache(&path, "fp\nline2"), Some(Verdict::Software));
        assert_eq!(
            read_cache(&path, "fp\nchanged"),
            None,
            "fingerprint changed"
        );

        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(read_cache(&path, "fp"), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn isolated_probe_reports_success_and_egl_errors() {
        assert_eq!(run_isolated(|| 0, PROBE_TIMEOUT), ProbeOutcome::Ok);
        // EGL_BAD_ALLOC is 0x3003 — the error from issue #237.
        assert_eq!(
            run_isolated(|| INIT_FAILED_BASE + 3, PROBE_TIMEOUT),
            ProbeOutcome::Failed("eglInitialize failed: EGL_BAD_ALLOC".into())
        );
    }

    #[test]
    fn isolated_probe_survives_a_crashing_driver() {
        // What WebKit does on this failure: abort().
        assert_eq!(
            run_isolated(|| std::process::abort(), PROBE_TIMEOUT),
            ProbeOutcome::Crashed(libc::SIGABRT)
        );
    }

    #[test]
    fn isolated_probe_times_out_a_hanging_driver() {
        let started = Instant::now();
        let outcome = run_isolated(
            || loop {
                std::thread::sleep(Duration::from_secs(60));
            },
            Duration::from_millis(200),
        );
        assert_eq!(outcome, ProbeOutcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// Smoke-test the real probes on the host: the dlopen/dlsym path and the
    /// EGL signatures. Any clean verdict is fine (CI has no GPU); a crash
    /// would point at a bad FFI signature rather than the driver.
    ///
    /// Unlike production (which forks before any thread exists), these fork
    /// from the multi-threaded test harness before `dlopen`; the timeout bounds
    /// any loader-lock deadlock that could cause, so they can't hang CI.
    #[test]
    fn real_egl_probes_return_a_verdict() {
        for (stage, outcome) in [
            ("GPU", run_isolated(probe_egl, PROBE_TIMEOUT)),
            ("software", run_isolated(probe_egl_software, PROBE_TIMEOUT)),
        ] {
            eprintln!("{stage} EGL probe on this host: {outcome}");
            assert!(
                !matches!(outcome, ProbeOutcome::Crashed(_) | ProbeOutcome::Skipped(_)),
                "{stage}: {outcome}"
            );
        }
    }
}
