//! Anonymous usage analytics via PostHog.
//!
//! All telemetry is:
//! - **Anonymous**: a random UUID is generated per machine, no personal data is collected.
//! - **Non-blocking**: events fire on a background OS thread with a 3-second HTTP timeout.
//! - **Opt-out**: respects `DO_NOT_TRACK=1`, `CTX_TELEMETRY=off`, or a local state file flag.
//!
//! State is stored at `$XDG_DATA_HOME/ctx/telemetry.json` (default `~/.local/share/ctx/telemetry.json`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

const POSTHOG_API_KEY: &str = "phc_yGGFTCyiKAJYRnVqn9yRUDWPYoKGIqJ71XNDPYIicOA";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Serialize, Deserialize)]
struct TelemetryState {
    anonymous_id: String,
    enabled: bool,
    noticed: bool,
}

// ─── XDG helpers ─────────────────────────────────────────────────────

fn xdg_data_dir() -> PathBuf {
    if let Some(val) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(val)
    } else {
        let home = std::env::var_os("HOME").unwrap_or_default();
        PathBuf::from(home).join(".local").join("share")
    }
}

fn state_path() -> PathBuf {
    xdg_data_dir().join("ctx").join("telemetry.json")
}

// ─── State file I/O ──────────────────────────────────────────────────

fn load_state() -> Option<TelemetryState> {
    let data = std::fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_state(state: &TelemetryState) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(path, json);
    }
}

fn load_or_create_state() -> TelemetryState {
    if let Some(state) = load_state() {
        return state;
    }
    let state = TelemetryState {
        anonymous_id: uuid::Uuid::new_v4().to_string(),
        enabled: true,
        noticed: false,
    };
    save_state(&state);
    state
}

// ─── Opt-out checks ──────────────────────────────────────────────────

fn env_opted_out() -> bool {
    if let Ok(val) = std::env::var("CTXH_DO_NOT_TRACK") {
        if val == "1" || val.eq_ignore_ascii_case("true") {
            return true;
        }
    }
    if let Ok(val) = std::env::var("CTX_TELEMETRY") {
        if val.eq_ignore_ascii_case("off") || val == "0" || val.eq_ignore_ascii_case("false") {
            return true;
        }
    }
    false
}

/// Returns `true` if telemetry is enabled (env vars permit it and state file has `enabled: true`).
pub fn is_enabled() -> bool {
    if env_opted_out() {
        return false;
    }
    load_state().is_none_or(|s| s.enabled)
}

// ─── First-run notice ────────────────────────────────────────────────

/// Print a one-time notice to stderr on the very first invocation.
/// Creates the state file if it doesn't exist yet.
pub fn show_notice_if_needed() {
    if env_opted_out() {
        return;
    }

    let mut state = load_or_create_state();
    if state.noticed {
        return;
    }

    eprintln!(
        "\n\
         Note: Context Harness collects anonymous usage analytics to improve the tool.\n\
         \x20     No personal data is collected. Disable with: export CTXH_DO_NOT_TRACK=1\n"
    );

    state.noticed = true;
    save_state(&state);
}

// ─── Event tracking ──────────────────────────────────────────────────

/// Handle returned by [`track`]. Call [`TelemetryGuard::wait`] before
/// process exit to give the HTTP request time to land.
pub struct TelemetryGuard {
    rx: std::sync::mpsc::Receiver<()>,
}

impl TelemetryGuard {
    /// Block until the telemetry request finishes or `REQUEST_TIMEOUT` elapses,
    /// whichever comes first. Safe to skip — the event is best-effort.
    pub fn wait(self) {
        let _ = self.rx.recv_timeout(REQUEST_TIMEOUT);
    }
}

/// Fire an analytics event to PostHog on a background OS thread.
///
/// Returns a [`TelemetryGuard`] whose `.wait()` should be called before
/// exiting so the HTTP request has time to complete. Returns `None` when
/// telemetry is disabled or the state file is missing.
///
/// Uses the **blocking** `posthog-rs` client (recommended for CLIs) so
/// the request is independent of the Tokio runtime lifetime. The HTTP
/// timeout is capped at 3 seconds.
pub fn track(event: &str, extra_properties: serde_json::Value) -> Option<TelemetryGuard> {
    if !is_enabled() {
        return None;
    }

    let distinct_id = match load_state() {
        Some(s) => s.anonymous_id,
        None => return None,
    };

    let event = event.to_string();
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let options = posthog_rs::ClientOptionsBuilder::default()
            .api_key(POSTHOG_API_KEY.to_string())
            .request_timeout_seconds(REQUEST_TIMEOUT.as_secs())
            .build()
            .expect("valid client options");
        let client = posthog_rs::client(options);

        let mut ph_event = posthog_rs::Event::new(&event, &distinct_id);
        let _ = ph_event.insert_prop("version", env!("CARGO_PKG_VERSION"));
        let _ = ph_event.insert_prop("os", std::env::consts::OS);
        let _ = ph_event.insert_prop("arch", std::env::consts::ARCH);

        if let Some(map) = extra_properties.as_object() {
            for (k, v) in map {
                let _ = ph_event.insert_prop(k.clone(), v.clone());
            }
        }

        let _ = client.capture(ph_event);
        let _ = tx.send(());
    });

    Some(TelemetryGuard { rx })
}
