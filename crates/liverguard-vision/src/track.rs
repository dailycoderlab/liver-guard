//! Per target hit memory.
//!
//! Two jobs: skip matching entirely when the screen has not changed since the
//! last hit (idle game screens are the common case), and give the
//! disambiguation policy a reference point so "the second identical button"
//! resolves to the one that was clicked last time.

use std::collections::HashMap;

use liverguard_ir::Rect;

#[derive(Debug, Clone)]
pub struct TrackEntry {
    pub frame_hash: String,
    pub rect: Rect,
    pub score: f64,
    pub point: (f64, f64),
}

#[derive(Debug, Default)]
pub struct Tracker {
    entries: HashMap<String, TrackEntry>,
    pub hits: u64,
    pub reuses: u64,
}

impl Tracker {
    pub fn remember(&mut self, target_id: &str, frame_hash: &str, rect: Rect, score: f64, point: (f64, f64)) {
        self.entries.insert(
            target_id.to_string(),
            TrackEntry { frame_hash: frame_hash.to_string(), rect, score, point },
        );
        self.hits += 1;
    }

    pub fn reusable(&mut self, target_id: &str, frame_hash: &str) -> Option<TrackEntry> {
        let entry = self.entries.get(target_id)?;
        if entry.frame_hash != frame_hash {
            return None;
        }
        self.reuses += 1;
        Some(entry.clone())
    }

    pub fn last_rect(&self, target_id: &str) -> Option<Rect> {
        self.entries.get(target_id).map(|e| e.rect)
    }

    pub fn forget(&mut self, target_id: &str) {
        self.entries.remove(target_id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
