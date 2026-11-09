//! Request / event log (worker **k**; classic `server/src/logs.mjs`): a bounded ring buffer (`CAPACITY` entries) of
//! `LogEntry` with a monotonic cursor, served by `GET /logs?after=` and by the app's Server page
//! (`Backend::server_logs`). Entries never contain tokens, cookies or full media URLs (host only).

use std::collections::VecDeque;
use std::sync::Mutex;

pub const CAPACITY: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub at_unix: i64,
    /// `request`, `source_completed`, `source_failed`, `pairing`, `revoked`, ...
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Default)]
pub struct LogBuffer {
    inner: Mutex<(u64, VecDeque<LogEntry>)>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append; `entry.id` is assigned here.
    pub fn push(&self, mut entry: LogEntry) -> u64 {
        let Ok(mut g) = self.inner.lock() else { return 0 };
        g.0 += 1;
        entry.id = g.0;
        if g.1.len() >= CAPACITY {
            g.1.pop_front();
        }
        g.1.push_back(entry);
        g.0
    }

    /// Entries with `id > after` and the newest id (the next `after`).
    pub fn since(&self, after: u64) -> (Vec<LogEntry>, u64) {
        let Ok(g) = self.inner.lock() else { return (Vec::new(), after) };
        (g.1.iter().filter(|e| e.id > after).cloned().collect(), g.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(event: &str) -> LogEntry {
        LogEntry { id: 0, at_unix: 0, event: event.into(), route: None, name: None, count: None, duration_ms: None, error: None }
    }

    #[test]
    fn cursor_and_capacity() {
        let b = LogBuffer::new();
        for i in 0..(CAPACITY + 5) {
            b.push(entry(&format!("e{i}")));
        }
        let (all, cursor) = b.since(0);
        assert_eq!(all.len(), CAPACITY);
        assert_eq!(cursor, (CAPACITY + 5) as u64);
        let (newer, _) = b.since(cursor - 2);
        assert_eq!(newer.len(), 2);
    }
}
