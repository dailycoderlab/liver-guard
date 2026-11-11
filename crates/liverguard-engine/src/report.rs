//! Run reporting: per-step score aggregation, a durable log and live events.
//!
//! Scores are aggregated per (step, target) so a 30 minute run writes tens of
//! rows, not tens of thousands. The aggregate rows and the JSONL log are what
//! the local run history stores; `RunEvent`s drive the canvas highlighting.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeState {
    Running,
    Ok,
    Failed,
}

/// Something worth showing live while a script runs.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunEvent {
    #[serde(rename_all = "camelCase")]
    Log {
        level: String,
        event: String,
        payload: Value,
    },
    #[serde(rename_all = "camelCase")]
    Node {
        node_id: String,
        state: NodeState,
        #[serde(skip_serializing_if = "Option::is_none")]
        score: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        point: Option<(f64, f64)>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

pub type EventSink = Arc<dyn Fn(&RunEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub run_id: String,
    pub ts: u64,
    pub level: String,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone)]
struct Aggregate {
    step_id: String,
    target_id: String,
    n: u64,
    total: f64,
    min_score: f64,
    max_ms: u64,
    last: f64,
}

impl Aggregate {
    fn row(&self) -> Value {
        let avg = if self.n > 0 {
            self.total / self.n as f64
        } else {
            0.0
        };
        json!({
            "stepId": self.step_id,
            "targetId": self.target_id,
            "n": self.n,
            "avg": round4(avg),
            "min": round4(self.min_score),
            "maxMs": self.max_ms,
            "last": round4(self.last),
        })
    }
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct Reporter {
    pub run_id: String,
    aggregates: BTreeMap<(String, String), Aggregate>,
    lines: Vec<LogLine>,
    sink: Option<EventSink>,
}

impl Reporter {
    pub fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            aggregates: BTreeMap::new(),
            lines: Vec::new(),
            sink: None,
        }
    }

    /// Receive every log line and node transition as it happens.
    pub fn subscribe(&mut self, sink: EventSink) {
        self.sink = Some(sink);
    }

    /// Returns `false` when the subscriber panicked.
    pub fn emit(&self, event: RunEvent) -> bool {
        match &self.sink {
            // A broken subscriber (the UI) must never abort the script run.
            Some(sink) => {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(&event))).is_ok()
            }
            None => true,
        }
    }

    pub fn log(&mut self, level: &str, event: &str, payload: Value) {
        let has_payload = !payload.is_null() && payload.as_object().map_or(true, |o| !o.is_empty());
        self.lines.push(LogLine {
            run_id: self.run_id.clone(),
            ts: now_ms(),
            level: level.to_string(),
            event: event.to_string(),
            payload: has_payload.then(|| payload.clone()),
        });
        let delivered = self.emit(RunEvent::Log {
            level: level.to_string(),
            event: event.to_string(),
            payload,
        });
        if !delivered {
            // The subscriber failed; keep a visible trace instead of swallowing it.
            self.lines.push(LogLine {
                run_id: self.run_id.clone(),
                ts: now_ms(),
                level: "warn".to_string(),
                event: "log_subscriber_failed".to_string(),
                payload: None,
            });
        }
    }

    pub fn node(
        &self,
        node_id: &str,
        state: NodeState,
        score: Option<f64>,
        point: Option<(f64, f64)>,
        message: Option<String>,
    ) {
        self.emit(RunEvent::Node {
            node_id: node_id.to_string(),
            state,
            score,
            point,
            message,
        });
    }

    pub fn score(&mut self, step_id: &str, target_id: &str, score: f64, ms: f64) {
        let entry = self
            .aggregates
            .entry((step_id.to_string(), target_id.to_string()))
            .or_insert_with(|| Aggregate {
                step_id: step_id.to_string(),
                target_id: target_id.to_string(),
                n: 0,
                total: 0.0,
                min_score: 1.0,
                max_ms: 0,
                last: 0.0,
            });
        entry.n += 1;
        entry.total += score;
        entry.min_score = entry.min_score.min(score);
        entry.max_ms = entry.max_ms.max(ms as u64);
        entry.last = score;
    }

    pub fn lines(&self) -> &[LogLine] {
        &self.lines
    }

    pub fn summary_rows(&self) -> Vec<Value> {
        let mut rows: Vec<Value> = self.aggregates.values().map(Aggregate::row).collect();
        rows.sort_by(|a, b| {
            a["min"]
                .as_f64()
                .partial_cmp(&b["min"].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        rows
    }

    pub fn summary(&self) -> Value {
        let rows = self.summary_rows();
        let low: Vec<&Value> = rows
            .iter()
            .filter(|r| r["min"].as_f64().unwrap_or(1.0) < 0.82)
            .collect();
        json!({ "steps": rows, "worst": rows.first(), "low": low })
    }

    pub fn jsonl(&self) -> String {
        self.lines
            .iter()
            .filter_map(|line| serde_json::to_string(line).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
