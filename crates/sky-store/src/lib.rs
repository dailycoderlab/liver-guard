//! Local, atomically written scripts, template assets and run history.
//!
//! Layout under the root (default `~/.sky`):
//!
//! ```text
//! scripts/<id>.json   one script per file: { id, name, ir }
//! assets/<sha>.png    template crops, content addressed
//! runs/<id>.json      { id, record, logs }
//! ```
//!
//! There is no draft/published split and no sync: saving overwrites the file.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sky_ir::IrDoc;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptRecord {
    pub id: String,
    pub name: String,
    pub ir: IrDoc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptSummary {
    pub id: String,
    pub name: String,
}

pub struct LocalStore {
    root: PathBuf,
    lock: Mutex<()>,
}

/// `$SKY_HOME` when set, otherwise `~/.sky` (`%USERPROFILE%\.sky` on Windows).
pub fn default_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("SKY_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(custom);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    home.unwrap_or_else(|| PathBuf::from(".")).join(".sky")
}

impl LocalStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), lock: Mutex::new(()) }
    }

    pub fn open_default() -> Self {
        Self::new(default_root())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("assets")
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Ids are 32 lowercase hex digits; anything else could escape the directory.
    fn check_id(value: &str) -> Result<&str> {
        let ok = value.len() == 32 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if ok {
            Ok(value)
        } else {
            Err(StoreError::Invalid("无效的本地标识".into()))
        }
    }

    fn check_sha(value: &str) -> Result<&str> {
        let ok = value.len() == 64 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if ok {
            Ok(value)
        } else {
            Err(StoreError::Invalid("Invalid asset hash".into()))
        }
    }

    fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
        let parent = path.parent().ok_or_else(|| StoreError::Invalid("path has no parent".into()))?;
        fs::create_dir_all(parent)?;
        let temp = parent.join(format!(".sky-{}", uuid::Uuid::new_v4().simple()));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::File::create(&temp)?;
            file.write_all(data)?;
            file.sync_all()?;
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        Ok(result?)
    }

    fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
        Self::write_atomic(path, &serde_json::to_vec_pretty(value)?)
    }

    fn script_path(&self, id: &str) -> Result<PathBuf> {
        Ok(self.root.join("scripts").join(format!("{}.json", Self::check_id(id)?)))
    }

    // -------------------------------------------------------------- scripts

    pub fn list_scripts(&self) -> Result<Vec<ScriptSummary>> {
        let _guard = self.guard();
        let dir = self.root.join("scripts");
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut scripts = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            // One unreadable file must not hide every other script.
            if let Ok(record) = fs::read(&path).map_err(StoreError::from).and_then(|b| Ok(serde_json::from_slice::<ScriptRecord>(&b)?)) {
                scripts.push(ScriptSummary { id: record.id, name: record.name });
            }
        }
        scripts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
        Ok(scripts)
    }

    pub fn get_script(&self, id: &str) -> Result<ScriptRecord> {
        let _guard = self.guard();
        self.read_script(id)
    }

    fn read_script(&self, id: &str) -> Result<ScriptRecord> {
        let path = self.script_path(id)?;
        let bytes = fs::read(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => StoreError::NotFound(format!("script {id}")),
            _ => e.into(),
        })?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn create_script(&self, name: &str, mut ir: IrDoc) -> Result<ScriptRecord> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Invalid("脚本名称不能为空".into()));
        }
        ir.meta.name = name.to_string();
        let record = ScriptRecord { id: uuid::Uuid::new_v4().simple().to_string(), name: name.to_string(), ir };
        let _guard = self.guard();
        Self::write_json(&self.script_path(&record.id)?, &record)?;
        Ok(record)
    }

    pub fn save_script(&self, id: &str, ir: IrDoc) -> Result<()> {
        let _guard = self.guard();
        let mut record = self.read_script(id)?;
        record.ir = ir;
        record.ir.meta.name = record.name.clone();
        Self::write_json(&self.script_path(id)?, &record)
    }

    pub fn rename_script(&self, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Invalid("脚本名称不能为空".into()));
        }
        let _guard = self.guard();
        let mut record = self.read_script(id)?;
        record.name = name.to_string();
        record.ir.meta.name = name.to_string();
        Self::write_json(&self.script_path(id)?, &record)
    }

    pub fn delete_script(&self, id: &str) -> Result<()> {
        let _guard = self.guard();
        fs::remove_file(self.script_path(id)?).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => StoreError::NotFound(format!("script {id}")),
            _ => e.into(),
        })
    }

    // --------------------------------------------------------------- assets

    /// Store a PNG under its own SHA-256 and return the hash.
    pub fn put_asset(&self, payload: &[u8]) -> Result<String> {
        let sha = hex::encode(Sha256::digest(payload));
        let path = self.asset_path(&sha)?;
        let _guard = self.guard();
        if !path.exists() {
            Self::write_atomic(&path, payload)?;
        }
        Ok(sha)
    }

    /// Store an asset whose hash the caller already claims; rejects mismatches.
    pub fn put_asset_verified(&self, sha: &str, payload: &[u8]) -> Result<()> {
        Self::check_sha(sha)?;
        if hex::encode(Sha256::digest(payload)) != sha {
            return Err(StoreError::Invalid("素材校验失败".into()));
        }
        self.put_asset(payload).map(|_| ())
    }

    pub fn asset_path(&self, sha: &str) -> Result<PathBuf> {
        Ok(self.assets_dir().join(format!("{}.png", Self::check_sha(sha)?)))
    }

    pub fn read_asset(&self, sha: &str) -> Result<Vec<u8>> {
        fs::read(self.asset_path(sha)?).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => StoreError::NotFound(format!("asset {}", &sha[..sha.len().min(8)])),
            _ => e.into(),
        })
    }

    // ----------------------------------------------------------------- runs

    pub fn save_run(&self, record: Value, logs: &str) -> Result<String> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let _guard = self.guard();
        let body = serde_json::json!({ "id": id, "record": record, "logs": logs });
        Self::write_json(&self.root.join("runs").join(format!("{id}.json")), &body)?;
        Ok(id)
    }

    /// Run summaries, newest first.
    pub fn list_runs(&self) -> Result<Vec<Value>> {
        let _guard = self.guard();
        let dir = self.root.join("runs");
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut runs = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else { continue };
            let mut summary = value["record"].take();
            if let (Some(map), Some(id)) = (summary.as_object_mut(), value["id"].as_str()) {
                map.insert("id".into(), Value::String(id.to_string()));
                runs.push(summary);
            }
        }
        runs.sort_by(|a, b| b["startedAt"].as_u64().cmp(&a["startedAt"].as_u64()));
        Ok(runs)
    }

    pub fn get_run(&self, id: &str) -> Result<Value> {
        let _guard = self.guard();
        let path = self.root.join("runs").join(format!("{}.json", Self::check_id(id)?));
        let bytes = fs::read(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => StoreError::NotFound(format!("run {id}")),
            _ => e.into(),
        })?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, LocalStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn scripts_round_trip_and_sort_by_name() {
        let (_dir, store) = store();
        let b = store.create_script("Beta", IrDoc::empty("x")).unwrap();
        let a = store.create_script("alpha", IrDoc::empty("x")).unwrap();

        let names: Vec<_> = store.list_scripts().unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["alpha", "Beta"]);
        assert_eq!(store.get_script(&a.id).unwrap().ir.meta.name, "alpha");

        store.rename_script(&b.id, "Gamma").unwrap();
        assert_eq!(store.get_script(&b.id).unwrap().ir.meta.name, "Gamma");

        store.delete_script(&a.id).unwrap();
        assert!(matches!(store.get_script(&a.id), Err(StoreError::NotFound(_))));
    }

    #[test]
    fn save_overwrites_content_and_keeps_the_name() {
        let (_dir, store) = store();
        let record = store.create_script("One", IrDoc::empty("x")).unwrap();
        let mut ir = IrDoc::empty("renamed-in-editor");
        ir.vars.insert("n".into(), sky_ir::VarValue::Num(3.0));
        store.save_script(&record.id, ir).unwrap();
        let loaded = store.get_script(&record.id).unwrap();
        assert_eq!(loaded.ir.meta.name, "One");
        assert_eq!(loaded.ir.vars.len(), 1);
    }

    #[test]
    fn blank_names_and_hostile_ids_are_rejected() {
        let (_dir, store) = store();
        assert!(store.create_script("   ", IrDoc::empty("x")).is_err());
        assert!(store.get_script("../../etc/passwd").is_err());
        assert!(store.delete_script("ABCDEF").is_err());
    }

    #[test]
    fn assets_are_content_addressed_and_verified() {
        let (_dir, store) = store();
        let sha = store.put_asset(b"png-bytes").unwrap();
        assert_eq!(sha.len(), 64);
        assert_eq!(store.read_asset(&sha).unwrap(), b"png-bytes");
        // Storing the same bytes again is a no-op, not an error.
        assert_eq!(store.put_asset(b"png-bytes").unwrap(), sha);

        assert!(store.put_asset_verified(&sha, b"other").is_err());
        assert!(store.read_asset(&"0".repeat(64)).is_err());
        assert!(store.asset_path("../x").is_err());
    }

    #[test]
    fn runs_list_newest_first() {
        let (_dir, store) = store();
        store.save_run(serde_json::json!({ "startedAt": 1, "state": "done" }), "a").unwrap();
        let newest = store.save_run(serde_json::json!({ "startedAt": 9, "state": "failed" }), "b").unwrap();
        let runs = store.list_runs().unwrap();
        assert_eq!(runs[0]["id"], newest.as_str());
        assert_eq!(runs[0]["state"], "failed");
        assert_eq!(store.get_run(&newest).unwrap()["logs"], "b");
    }

    #[test]
    fn a_corrupt_script_does_not_hide_the_others() {
        let (dir, store) = store();
        store.create_script("fine", IrDoc::empty("x")).unwrap();
        fs::write(dir.path().join("scripts").join(format!("{}.json", "a".repeat(32))), b"{not json").unwrap();
        assert_eq!(store.list_scripts().unwrap().len(), 1);
    }
}
