//! Local, atomically written scripts, template assets and run history.
//!
//! Layout under the root (default `~/.liverguard`):
//!
//! ```text
//! scripts/<id>.json   one script per file: { id, name, ir }
//! assets/<sha>.png    template crops, content addressed
//! runs/<id>.json      { id, record, logs }
//! settings.json       app-wide settings (failure alerts); never part of a script or its export
//! ```
//!
//! There is no draft/published split and no sync: saving overwrites the file.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use std::collections::BTreeMap;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use liverguard_ir::IrDoc;
use liverguard_notify::NotifyConfig;

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

/// App-wide settings, kept apart from scripts: they belong to this machine (the webhook URL
/// carries a secret token) and must not travel with an exported script.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub notify: NotifyConfig,
}

/// `format` value of a script export file.
pub const BUNDLE_FORMAT: &str = "liverguard-script";
/// Bump when the export layout changes; import refuses anything newer.
pub const BUNDLE_VERSION: u32 = 1;

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A script together with the template images it needs, as one self-contained file.
///
/// `assets` maps the SHA-256 of each PNG (the name the script refers to it by) to its
/// base64 bytes, so the file moves between machines without `~/.liverguard/assets`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptBundle {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub ir: IrDoc,
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
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

/// `$LIVERGUARD_HOME` when set, otherwise `~/.liverguard` (`%USERPROFILE%\.liverguard` on Windows).
pub fn default_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("LIVERGUARD_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(custom);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    home.unwrap_or_else(|| PathBuf::from(".")).join(".liverguard")
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
        let temp = parent.join(format!(".liverguard-{}", uuid::Uuid::new_v4().simple()));
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

    // ------------------------------------------------------------- settings

    /// The saved settings, or the defaults when nothing was saved yet. A file that exists but
    /// cannot be read is an error: silently falling back would turn alerts off unnoticed.
    pub fn load_settings(&self) -> Result<Settings> {
        let _guard = self.guard();
        match fs::read(self.root.join("settings.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let _guard = self.guard();
        Self::write_json(&self.root.join("settings.json"), settings)
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

    /// Copy the stored script under a new id and name. Template assets are content addressed,
    /// so the copy shares them with the original and nothing under `assets/` is duplicated.
    pub fn duplicate_script(&self, id: &str, name: &str) -> Result<ScriptRecord> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Invalid("脚本名称不能为空".into()));
        }
        let _guard = self.guard();
        let mut record = self.read_script(id)?;
        record.id = uuid::Uuid::new_v4().simple().to_string();
        record.name = name.to_string();
        record.ir.meta.name = name.to_string();
        Self::write_json(&self.script_path(&record.id)?, &record)?;
        Ok(record)
    }

    /// Bundle the stored script with every template image it uses.
    pub fn export_script(&self, id: &str) -> Result<ScriptBundle> {
        let record = {
            let _guard = self.guard();
            self.read_script(id)?
        };
        let mut assets = BTreeMap::new();
        for sha in record.ir.asset_hashes() {
            if assets.contains_key(&sha) {
                continue;
            }
            let bytes = self.read_asset(&sha).map_err(|e| match e {
                StoreError::NotFound(_) => StoreError::Invalid(format!("模板素材 {} 缺失，无法导出", &sha[..sha.len().min(8)])),
                other => other,
            })?;
            assets.insert(sha, base64::engine::general_purpose::STANDARD.encode(bytes));
        }
        Ok(ScriptBundle { format: BUNDLE_FORMAT.into(), version: BUNDLE_VERSION, name: record.name, ir: record.ir, assets })
    }

    /// Parse an export file's bytes and import it (see `import_bundle`).
    pub fn import_bytes(&self, bytes: &[u8]) -> Result<ScriptRecord> {
        // Check the envelope first so a random JSON file gets a useful message.
        let value: Value = serde_json::from_slice(bytes).map_err(|_| StoreError::Invalid("不是有效的脚本文件（无法解析为 JSON）".into()))?;
        if value["format"] != BUNDLE_FORMAT {
            return Err(StoreError::Invalid("不是 LiverGuard 导出的脚本文件".into()));
        }
        let version = value["version"].as_u64().unwrap_or(0);
        if version == 0 || version > u64::from(BUNDLE_VERSION) {
            return Err(StoreError::Invalid(format!("脚本文件版本 {version} 不受支持，请升级 LiverGuard")));
        }
        let bundle: ScriptBundle = serde_json::from_value(value).map_err(|e| StoreError::Invalid(format!("脚本文件内容不完整：{e}")))?;
        self.import_bundle(bundle)
    }

    /// Store the bundle as a brand-new script (fresh id, so it never overwrites anything) and
    /// write its images. Everything is checked before the first byte is written. A name that is
    /// already taken gets an `（导入）` suffix.
    pub fn import_bundle(&self, bundle: ScriptBundle) -> Result<ScriptRecord> {
        if bundle.ir.ir_version != liverguard_ir::IR_VERSION {
            return Err(StoreError::Invalid(format!(
                "脚本的 IR 版本是 v{}，当前只支持 v{}",
                bundle.ir.ir_version,
                liverguard_ir::IR_VERSION
            )));
        }

        let mut decoded: Vec<(String, Vec<u8>)> = Vec::new();
        for (sha, text) in &bundle.assets {
            Self::check_sha(sha)?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(text.trim())
                .map_err(|_| StoreError::Invalid(format!("素材 {} 不是有效的 base64", &sha[..8])))?;
            if !bytes.starts_with(PNG_MAGIC) {
                return Err(StoreError::Invalid(format!("素材 {} 不是 PNG 图片", &sha[..8])));
            }
            if hex::encode(Sha256::digest(&bytes)) != *sha {
                return Err(StoreError::Invalid(format!("素材 {} 校验失败，文件可能已损坏", &sha[..8])));
            }
            decoded.push((sha.clone(), bytes));
        }
        for sha in bundle.ir.asset_hashes() {
            if !bundle.assets.contains_key(&sha) && !self.asset_path(&sha)?.exists() {
                return Err(StoreError::Invalid(format!("脚本用到的模板素材 {} 不在文件里", &sha[..sha.len().min(8)])));
            }
        }

        for (_, bytes) in &decoded {
            self.put_asset(bytes)?;
        }

        let base = {
            let name = bundle.name.trim();
            if name.is_empty() { "导入的脚本".to_string() } else { name.to_string() }
        };
        let taken: Vec<String> = self.list_scripts()?.into_iter().map(|s| s.name).collect();
        let mut name = base.clone();
        let mut n = 1;
        while taken.contains(&name) {
            n += 1;
            name = if n == 2 { format!("{base}（导入）") } else { format!("{base}（导入 {}）", n - 1) };
        }
        self.create_script(&name, bundle.ir)
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
        ir.meta.game = Some("g".into());
        store.save_script(&record.id, ir).unwrap();
        let loaded = store.get_script(&record.id).unwrap();
        assert_eq!(loaded.ir.meta.name, "One");
        assert_eq!(loaded.ir.meta.game.as_deref(), Some("g"));
    }

    #[test]
    fn duplicating_copies_the_content_under_a_new_id_and_name() {
        let (_dir, store) = store();
        let original = store.create_script("One", IrDoc::empty("x")).unwrap();
        let mut ir = original.ir.clone();
        ir.meta.game = Some("g".into());
        store.save_script(&original.id, ir).unwrap();

        let copy = store.duplicate_script(&original.id, "  One 副本 ").unwrap();
        assert_ne!(copy.id, original.id);
        assert_eq!(copy.name, "One 副本");
        assert_eq!(copy.ir.meta.name, "One 副本");
        assert_eq!(store.get_script(&copy.id).unwrap().ir.meta.game.as_deref(), Some("g"));

        // The original is untouched and both are listed.
        assert_eq!(store.get_script(&original.id).unwrap().name, "One");
        assert_eq!(store.list_scripts().unwrap().len(), 2);

        // Editing the copy does not leak into the original.
        store.rename_script(&copy.id, "Other").unwrap();
        assert_eq!(store.get_script(&original.id).unwrap().ir.meta.name, "One");

        assert!(store.duplicate_script(&original.id, "  ").is_err());
        assert!(matches!(store.duplicate_script(&"a".repeat(32), "x"), Err(StoreError::NotFound(_))));
        assert!(store.duplicate_script("../x", "x").is_err());
    }

    /// A script that uses one template image, stored in `store`.
    fn script_with_template(store: &LocalStore, name: &str) -> (ScriptRecord, String) {
        let mut png = PNG_MAGIC.to_vec();
        png.extend_from_slice(name.as_bytes());
        let sha = store.put_asset(&png).unwrap();
        let mut ir = IrDoc::empty(name);
        ir.targets.insert(
            "ok".into(),
            liverguard_ir::Target::Template(liverguard_ir::TemplateTarget::new(sha.clone(), liverguard_ir::AssetBox { w: 10.0, h: 10.0 }, [0.0, 0.0, 1.0, 1.0])),
        );
        (store.create_script(name, ir).unwrap(), sha)
    }

    #[test]
    fn a_script_exports_with_its_images_and_imports_on_another_store() {
        let (_a, source) = store();
        let (_b, target) = store();
        let (record, sha) = script_with_template(&source, "Quest");
        // An image no script uses must not ride along.
        source.put_asset(b"unused").unwrap();

        let bundle = source.export_script(&record.id).unwrap();
        assert_eq!(bundle.format, BUNDLE_FORMAT);
        assert_eq!(bundle.assets.keys().collect::<Vec<_>>(), vec![&sha]);

        let bytes = serde_json::to_vec(&bundle).unwrap();
        let imported = target.import_bytes(&bytes).unwrap();
        assert_ne!(imported.id, record.id);
        assert_eq!(imported.name, "Quest");
        assert_eq!(imported.ir.meta.name, "Quest");
        assert_eq!(imported.ir.targets.len(), 1);
        assert_eq!(target.read_asset(&sha).unwrap(), source.read_asset(&sha).unwrap());
        assert_eq!(target.list_scripts().unwrap().len(), 1);
    }

    #[test]
    fn importing_never_overwrites_and_renames_a_taken_name() {
        let (_dir, store) = store();
        let (record, _) = script_with_template(&store, "Quest");
        let bytes = serde_json::to_vec(&store.export_script(&record.id).unwrap()).unwrap();

        let first = store.import_bytes(&bytes).unwrap();
        let second = store.import_bytes(&bytes).unwrap();
        assert_eq!(first.name, "Quest（导入）");
        assert_eq!(second.name, "Quest（导入 2）");
        assert_eq!(store.list_scripts().unwrap().len(), 3);
        assert_eq!(store.get_script(&record.id).unwrap().name, "Quest");
    }

    #[test]
    fn a_damaged_or_foreign_file_is_refused_and_writes_nothing() {
        let (_a, source) = store();
        let (dir, target) = store();
        let (record, sha) = script_with_template(&source, "Quest");
        let good = source.export_script(&record.id).unwrap();

        assert!(target.import_bytes(b"not json").is_err());
        assert!(target.import_bytes(br#"{"hello": 1}"#).is_err());

        let mut newer = good.clone();
        newer.version = BUNDLE_VERSION + 1;
        assert!(target.import_bytes(&serde_json::to_vec(&newer).unwrap()).is_err());

        // Image bytes that do not match the hash they are filed under.
        let mut tampered = good.clone();
        tampered.assets.insert(sha.clone(), base64::engine::general_purpose::STANDARD.encode([PNG_MAGIC, b"other"].concat()));
        assert!(target.import_bytes(&serde_json::to_vec(&tampered).unwrap()).is_err());

        // The script needs an image the file does not carry (and the store lacks).
        let mut missing = good.clone();
        missing.assets.clear();
        assert!(target.import_bytes(&serde_json::to_vec(&missing).unwrap()).is_err());

        // An old IR version.
        let mut old = good;
        old.ir.ir_version = 2;
        assert!(target.import_bytes(&serde_json::to_vec(&old).unwrap()).is_err());

        assert!(target.list_scripts().unwrap().is_empty());
        assert!(!dir.path().join("assets").exists());
    }

    #[test]
    fn exporting_a_script_whose_image_is_gone_says_so() {
        let (_dir, store) = store();
        let (record, sha) = script_with_template(&store, "Quest");
        fs::remove_file(store.asset_path(&sha).unwrap()).unwrap();
        let error = store.export_script(&record.id).unwrap_err();
        assert!(error.to_string().contains("缺失"), "{error}");
    }

    #[test]
    fn settings_default_to_off_and_round_trip() {
        let (dir, store) = store();
        assert_eq!(store.load_settings().unwrap(), Settings::default());
        assert!(!store.load_settings().unwrap().notify.is_enabled());

        let mut settings = Settings::default();
        settings.notify.url = "https://example.com/hook".into();
        settings.notify.channel = liverguard_notify::Channel::Feishu;
        store.save_settings(&settings).unwrap();
        assert_eq!(store.load_settings().unwrap(), settings);

        // Alerts are never part of a script file or its export.
        let record = store.create_script("One", IrDoc::empty("x")).unwrap();
        let exported = serde_json::to_string(&store.export_script(&record.id).unwrap()).unwrap();
        assert!(!exported.contains("example.com"));

        // A damaged file is an error, not a silent "alerts off".
        fs::write(dir.path().join("settings.json"), b"{broken").unwrap();
        assert!(store.load_settings().is_err());
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
