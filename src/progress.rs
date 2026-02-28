//! Sync and embed progress reporting.
//!
//! Reports observable progress during `ctx sync` (and optionally `ctx embed pending`)
//! so users see what is being scanned, how much is left, and when search is up to date.
//! Progress is emitted on **stderr** so stdout remains parseable for scripts.
//!
//! See [SYNC_PROGRESS.md](../docs/SYNC_PROGRESS.md) for the design.

use std::io::Write;

/// Phase of the sync pipeline (used in JSON output and for future extensions).
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncPhase {
    /// Connector is scanning (e.g. walking filesystem, listing Git). Total unknown.
    Discovering,
    /// Items are being upserted, chunked, and optionally embedded. Counts known.
    Ingesting,
}

/// A single progress event for sync.
#[derive(Clone, Debug)]
pub enum SyncProgressEvent {
    /// Currently in discovery phase for this connector (no total yet).
    Discovering { connector: String },
    /// Ingest phase: n items processed out of total.
    Ingesting {
        connector: String,
        n: u64,
        total: u64,
    },
}

/// Reports sync progress. Implementations write to stderr (human or JSON).
pub trait SyncProgressReporter: Send + Sync {
    /// Emit a progress event. Called from the ingest pipeline.
    fn report(&self, event: SyncProgressEvent);
}

/// Human-friendly progress on stderr: "sync filesystem:docs  ingesting  1,234 / 5,000 items".
pub struct StderrProgress;

impl SyncProgressReporter for StderrProgress {
    fn report(&self, event: SyncProgressEvent) {
        let line = match &event {
            SyncProgressEvent::Discovering { connector } => {
                format!("sync {}  discovering...\n", connector)
            }
            SyncProgressEvent::Ingesting {
                connector,
                n,
                total,
            } => {
                let n_fmt = format_number(*n);
                let total_fmt = format_number(*total);
                format!(
                    "sync {}  ingesting  {} / {} items\n",
                    connector, n_fmt, total_fmt
                )
            }
        };
        let _ = std::io::stderr().lock().write_all(line.as_bytes());
        let _ = std::io::stderr().lock().flush();
    }
}

/// Machine-readable progress: one JSON object per line on stderr.
pub struct JsonProgress;

impl SyncProgressReporter for JsonProgress {
    fn report(&self, event: SyncProgressEvent) {
        let obj = match &event {
            SyncProgressEvent::Discovering { connector } => serde_json::json!({
                "event": "progress",
                "connector": connector,
                "phase": "discovering"
            }),
            SyncProgressEvent::Ingesting {
                connector,
                n,
                total,
            } => serde_json::json!({
                "event": "progress",
                "connector": connector,
                "phase": "ingesting",
                "n": n,
                "total": total
            }),
        };
        if let Ok(line) = serde_json::to_string(&obj) {
            let _ = writeln!(std::io::stderr().lock(), "{}", line);
            let _ = std::io::stderr().lock().flush();
        }
    }
}

/// No-op reporter when progress is disabled.
pub struct NoProgress;

impl SyncProgressReporter for NoProgress {
    fn report(&self, _event: SyncProgressEvent) {}
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + (s.len() - 1) / 3);
    let chars: Vec<char> = s.chars().rev().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(*c);
    }
    result.chars().rev().collect()
}

/// Progress mode for the CLI: off, human (stderr), or JSON (stderr).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressMode {
    Off,
    Human,
    Json,
}

impl ProgressMode {
    /// Default: human progress when stderr is a TTY, otherwise off.
    pub fn default_for_tty() -> Self {
        if atty::is(atty::Stream::Stderr) {
            ProgressMode::Human
        } else {
            ProgressMode::Off
        }
    }

    /// Build a reporter for this mode. Caller can pass it to ingest.
    pub fn reporter(&self) -> Box<dyn SyncProgressReporter> {
        match self {
            ProgressMode::Off => Box::new(NoProgress),
            ProgressMode::Human => Box::new(StderrProgress),
            ProgressMode::Json => Box::new(JsonProgress),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_number_comma() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(1), "1");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1234), "1,234");
        assert_eq!(format_number(1_234_567), "1,234,567");
    }
}
