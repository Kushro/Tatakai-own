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
    // ---- F6 (parity wave B) ----

    #[test]
    fn ids_are_assigned_in_order_and_ignore_the_input_id() {
        let b = LogBuffer::new();
        let mut e = entry("a");
        e.id = 999;
        assert_eq!(b.push(e), 1);
        assert_eq!(b.push(entry("b")), 2);
        let (all, cursor) = b.since(0);
        assert_eq!(cursor, 2);
        assert_eq!(all.iter().map(|e| e.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(all[0].event, "a");
    }

    #[test]
    fn since_the_cursor_or_beyond_is_empty() {
        let b = LogBuffer::new();
        assert_eq!(b.since(0), (Vec::new(), 0));
        b.push(entry("a"));
        let (none, cursor) = b.since(1);
        assert!(none.is_empty());
        assert_eq!(cursor, 1);
        assert!(b.since(50).0.is_empty());
    }

    #[test]
    fn capacity_drops_the_oldest() {
        let b = LogBuffer::new();
        for i in 0..(CAPACITY + 3) {
            b.push(entry(&format!("e{i}")));
        }
        let (all, _) = b.since(0);
        assert_eq!(all.first().unwrap().event, "e3");
        assert_eq!(all.first().unwrap().id, 4);
        assert_eq!(all.last().unwrap().event, format!("e{}", CAPACITY + 2));
    }

    #[test]
    fn entries_serialize_camel_case_without_empty_fields() {
        let mut e = entry("request");
        e.duration_ms = Some(12);
        e.route = Some("/sources".into());
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["durationMs"], 12);
        assert_eq!(v["atUnix"], 0);
        assert_eq!(v["route"], "/sources");
        assert!(v.get("name").is_none() && v.get("count").is_none() && v.get("error").is_none());
    }
}
