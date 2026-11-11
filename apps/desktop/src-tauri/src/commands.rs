//! Tauri commands: the contract in `apps/desktop/web/src/api/types.ts`.
//!
//! Everything that touches the screen or the matcher runs off the main thread;
//! a sync command would block the window. `Frame` is `!Send` (it caches), so
//! frames never cross threads: only the pixel `Mat` and its bounds do.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use liverguard_engine::{Driver, DriverError, EventSink, ProbeRow};
use liverguard_ir::{Bounds, IrDoc, Problem, Rect, Target};
use liverguard_notify::NotifyConfig;
use liverguard_store::{LocalStore, ScriptRecord, ScriptSummary};
use liverguard_vision::detect::{detect_candidates as detect, DetectOptions, PixelBox};
use liverguard_vision::{author, AssetStore, Frame};
use opencv::core::Mat;
use opencv::prelude::*;
use serde_json::{json, Value};
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::platform::{self, PermissionInfo, Surface, WindowRow};

type CmdResult<T> = Result<T, String>;

const LIVE_INTERVAL: Duration = Duration::from_millis(120);
const LIVE_JPEG_QUALITY: i32 = 72;
const LIVE_MAX_WIDTH: i32 = 1280;
const HEADER_BYTES: usize = 48;
const MAX_IMAGE_BYTES: usize = 30 * 1024 * 1024;

/// The frame the user froze; `detect_candidates` and `create_template` work on
/// exactly these pixels, matched by sequence number.
struct Pinned {
    seq: u64,
    image: Mat,
    bounds: Bounds,
    /// A user-supplied image rather than a capture: there is no screen under
    /// it, so the accessibility lookup must be skipped.
    external: bool,
}

struct LiveHandle {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

pub struct AppState {
    store: Arc<LocalStore>,
    seq: AtomicU64,
    surface: Mutex<Surface>,
    pinned: Mutex<Option<Pinned>>,
    live: Mutex<Option<LiveHandle>>,
    run_slot: Arc<Mutex<Option<Arc<AtomicBool>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            store: Arc::new(LocalStore::open_default()),
            seq: AtomicU64::new(1),
            surface: Mutex::new(Surface::FullScreen),
            pinned: Mutex::new(None),
            live: Mutex::new(None),
            run_slot: Arc::new(Mutex::new(None)),
        }
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    fn stop_live(&self) {
        let handle = self.live.lock().unwrap().take();
        if let Some(handle) = handle {
            handle.stop.store(true, Ordering::Relaxed);
            let _ = handle.thread.join();
        }
    }
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// `seq u64 | x y w h scale f64 | payload`, little endian.
fn packet(seq: u64, bounds: &Bounds, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_BYTES + payload.len());
    out.extend_from_slice(&seq.to_le_bytes());
    for value in [bounds.x, bounds.y, bounds.w, bounds.h, bounds.scale] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(payload);
    out
}

fn capture_retrying(driver: &mut dyn Driver) -> Result<Frame, DriverError> {
    let mut last = DriverError::SurfaceChanged;
    for _ in 0..3 {
        match driver.capture() {
            Ok(frame) => return Ok(frame),
            Err(DriverError::SurfaceChanged) => {
                last = DriverError::SurfaceChanged;
                std::thread::sleep(Duration::from_millis(80));
            }
            Err(other) => return Err(other),
        }
    }
    Err(last)
}

fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> impl std::future::Future<Output = CmdResult<T>> {
    async move {
        tauri::async_runtime::spawn_blocking(work)
            .await
            .map_err(err)?
    }
}

// ------------------------------------------------------------------ scripts

#[tauri::command]
pub fn list_scripts(state: State<'_, AppState>) -> CmdResult<Vec<ScriptSummary>> {
    state.store.list_scripts().map_err(err)
}

#[tauri::command]
pub fn get_script(state: State<'_, AppState>, id: String) -> CmdResult<ScriptRecord> {
    state.store.get_script(&id).map_err(err)
}

#[tauri::command]
pub fn create_script(state: State<'_, AppState>, name: String) -> CmdResult<ScriptRecord> {
    state
        .store
        .create_script(&name, IrDoc::empty(&name))
        .map_err(err)
}

#[tauri::command]
pub fn save_script(state: State<'_, AppState>, id: String, ir: IrDoc) -> CmdResult<()> {
    state.store.save_script(&id, ir).map_err(err)
}

#[tauri::command]
pub fn rename_script(state: State<'_, AppState>, id: String, name: String) -> CmdResult<()> {
    state.store.rename_script(&id, &name).map_err(err)
}

#[tauri::command]
pub fn duplicate_script(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> CmdResult<ScriptRecord> {
    state.store.duplicate_script(&id, &name).map_err(err)
}

/// Largest script file we will read: images are embedded, but a runaway file is still a mistake.
const MAX_IMPORT_BYTES: u64 = 200 * 1024 * 1024;

/// A file name that is safe on macOS and Windows, built from the script name.
fn export_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.');
    format!(
        "{}.liverguard.json",
        if cleaned.is_empty() {
            "script"
        } else {
            cleaned
        }
    )
}

/// Save the script and its template images to a file the user picks. Resolves the path written,
/// or `None` when the dialog was cancelled.
#[tauri::command]
pub async fn export_script(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<Option<String>> {
    let store = state.store.clone();
    blocking(move || {
        let bundle = store.export_script(&id).map_err(err)?;
        let picked = app
            .dialog()
            .file()
            .add_filter("LiverGuard 脚本", &["json"])
            .set_file_name(export_file_name(&bundle.name))
            .blocking_save_file();
        let Some(picked) = picked else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(err)?;
        let bytes = serde_json::to_vec_pretty(&bundle).map_err(err)?;
        std::fs::write(&path, bytes).map_err(err)?;
        Ok(Some(path.display().to_string()))
    })
    .await
}

/// Pick an exported script file and add it as a new script. `None` when cancelled.
#[tauri::command]
pub async fn import_script(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<Option<ScriptRecord>> {
    let store = state.store.clone();
    blocking(move || {
        let picked = app
            .dialog()
            .file()
            .add_filter("LiverGuard 脚本", &["json"])
            .blocking_pick_file();
        let Some(picked) = picked else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(err)?;
        if std::fs::metadata(&path).map_err(err)?.len() > MAX_IMPORT_BYTES {
            return Err("文件过大，不像是导出的脚本".to_string());
        }
        let bytes = std::fs::read(&path).map_err(err)?;
        store.import_bytes(&bytes).map(Some).map_err(err)
    })
    .await
}

#[tauri::command]
pub fn delete_script(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.store.delete_script(&id).map_err(err)
}

#[tauri::command]
pub fn validate_ir(ir: IrDoc) -> Vec<Problem> {
    ir.validate()
}

// ----------------------------------------------------------------- settings

#[tauri::command]
pub fn get_notify_config(state: State<'_, AppState>) -> CmdResult<NotifyConfig> {
    state.store.load_settings().map(|s| s.notify).map_err(err)
}

/// Save the alert settings. Alerts are on exactly when a webhook URL is set, so an empty URL
/// (alerts off) is always fine; a set one has to be usable.
#[tauri::command]
pub fn save_notify_config(state: State<'_, AppState>, config: NotifyConfig) -> CmdResult<()> {
    if config.is_enabled() {
        liverguard_notify::validate(&config).map_err(err)?;
    }
    let mut settings = state.store.load_settings().map_err(err)?;
    settings.notify = config;
    state.store.save_settings(&settings).map_err(err)
}

/// Send a test message with the config as typed in the form (not necessarily saved yet).
#[tauri::command]
pub async fn send_test_notification(config: NotifyConfig) -> CmdResult<()> {
    blocking(move || {
        liverguard_notify::send(&config, &liverguard_notify::test_message()).map_err(err)
    })
    .await
}

// ----------------------------------------------------------------- platform

#[tauri::command]
pub async fn list_windows() -> CmdResult<Vec<WindowRow>> {
    blocking(|| Ok(platform::list_windows())).await
}

#[tauri::command]
pub fn permissions() -> PermissionInfo {
    platform::permissions()
}

#[tauri::command]
pub async fn request_permissions() -> CmdResult<PermissionInfo> {
    blocking(|| Ok(platform::request_permissions())).await
}

#[tauri::command]
pub fn open_permission_settings(kind: String) {
    platform::open_permission_settings(&kind);
}

// --------------------------------------------------------------------- live

#[tauri::command]
pub async fn live_start(
    state: State<'_, AppState>,
    surface: Surface,
    on_frame: Channel<InvokeResponseBody>,
) -> CmdResult<()> {
    state.stop_live();
    *state.surface.lock().unwrap() = surface;

    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("liverguard-live".into())
        .spawn(move || live_loop(surface, flag, on_frame))
        .map_err(err)?;
    *state.live.lock().unwrap() = Some(LiveHandle { stop, thread });
    Ok(())
}

fn live_loop(surface: Surface, stop: Arc<AtomicBool>, channel: Channel<InvokeResponseBody>) {
    let mut driver = platform::make_driver(surface);
    let mut seq = 0u64;
    let report = |message: String| {
        let _ = channel.send(InvokeResponseBody::Json(
            json!({ "kind": "error", "message": message }).to_string(),
        ));
    };

    while !stop.load(Ordering::Relaxed) {
        let started = std::time::Instant::now();
        match driver.capture() {
            Ok(frame) => match frame.to_jpeg(LIVE_JPEG_QUALITY, LIVE_MAX_WIDTH) {
                Ok(jpeg) => {
                    seq += 1;
                    if channel
                        .send(InvokeResponseBody::Raw(packet(seq, &frame.bounds, &jpeg)))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => return report(error.to_string()),
            },
            Err(DriverError::SurfaceChanged) => {}
            Err(error) => return report(error.to_string()),
        }
        if let Some(rest) = LIVE_INTERVAL.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}

#[tauri::command]
pub async fn live_stop(state: State<'_, AppState>) -> CmdResult<()> {
    state.stop_live();
    Ok(())
}

// ---------------------------------------------------------------- snapshot

/// Captures one full-resolution frame, pins it, and returns it as PNG.
#[tauri::command]
pub async fn snapshot(state: State<'_, AppState>) -> CmdResult<Response> {
    let surface = *state.surface.lock().unwrap();
    let seq = state.next_seq();
    let (png, bounds, image) = blocking(move || {
        let mut driver = platform::make_driver(surface);
        let frame = capture_retrying(&mut *driver).map_err(err)?;
        let png = frame.to_png().map_err(err)?;
        Ok((png, frame.bounds, frame.image.try_clone().map_err(err)?))
    })
    .await?;
    let reply = packet(seq, &bounds, &png);
    *state.pinned.lock().unwrap() = Some(Pinned {
        seq,
        image,
        bounds,
        external: false,
    });
    Ok(Response::new(reply))
}

/// Pins a user-supplied screenshot in place of a capture. The request body is
/// the raw file; the reply has the same layout as `snapshot`.
#[tauri::command]
pub async fn load_image(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> CmdResult<Response> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("需要图片的原始字节".to_string());
    };
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!("图片超过 {} MB", MAX_IMAGE_BYTES / 1024 / 1024));
    }
    let bytes = bytes.clone();
    state.stop_live();
    let seq = state.next_seq();
    let (png, bounds, image) = blocking(move || {
        let (image, bounds) = author::decode_image(&bytes).map_err(err)?;
        let png = Frame::new(image.try_clone().map_err(err)?, bounds)
            .to_png()
            .map_err(err)?;
        Ok((png, bounds, image))
    })
    .await?;
    let reply = packet(seq, &bounds, &png);
    *state.pinned.lock().unwrap() = Some(Pinned {
        seq,
        image,
        bounds,
        external: true,
    });
    Ok(Response::new(reply))
}

fn pinned_frame(state: &AppState, seq: u64) -> CmdResult<(Mat, Bounds, bool)> {
    let guard = state.pinned.lock().unwrap();
    match guard.as_ref() {
        Some(pinned) if pinned.seq == seq => Ok((
            pinned.image.try_clone().map_err(err)?,
            pinned.bounds,
            pinned.external,
        )),
        _ => Err("画面已过期，请重新冻结后再操作".to_string()),
    }
}

#[tauri::command]
pub async fn detect_candidates(
    state: State<'_, AppState>,
    seq: u64,
    x: f64,
    y: f64,
) -> CmdResult<Vec<Rect>> {
    let (image, bounds, external) = pinned_frame(&state, seq)?;
    let surface = *state.surface.lock().unwrap();
    blocking(move || {
        // The native accessibility box is a bonus, never a requirement; an uploaded image has no screen to ask.
        let ax_box = if external {
            None
        } else {
            let (gx, gy) = platform::pixel_to_global(&bounds, x, y);
            platform::make_driver(surface)
                .ax_at(gx, gy)
                .ok()
                .flatten()
                .map(|r| PixelBox {
                    x: (r.x - bounds.x) * bounds.scale,
                    y: (r.y - bounds.y) * bounds.scale,
                    w: r.w * bounds.scale,
                    h: r.h * bounds.scale,
                })
        };
        let options = DetectOptions {
            scale: bounds.scale,
            ax_box,
            ..DetectOptions::default()
        };
        let found = detect(&image, (x, y), &options).map_err(err)?;
        Ok(found
            .into_iter()
            .map(|c| Rect::new(c.x, c.y, c.w, c.h))
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn create_template(
    state: State<'_, AppState>,
    seq: u64,
    rect: Rect,
) -> CmdResult<Target> {
    let (image, bounds, _) = pinned_frame(&state, seq)?;
    let store = state.store.clone();
    blocking(move || {
        let frame = Frame::new(image, bounds);
        let area = (
            rect.x.round() as i32,
            rect.y.round() as i32,
            rect.w.round() as i32,
            rect.h.round() as i32,
        );
        let created = author::create_template(&frame, area).map_err(err)?;
        store
            .put_asset_verified(&created.target.asset, &created.png)
            .map_err(err)?;
        Ok(Target::Template(created.target))
    })
    .await
}

/// Cut a smaller template out of an existing template target's image. `rect` is in pixels of
/// that image. Returns the target with the new asset; the old asset stays in the store, so
/// undoing in the editor still finds it.
#[tauri::command]
pub async fn refine_template(
    state: State<'_, AppState>,
    target: Target,
    rect: Rect,
) -> CmdResult<Target> {
    let Target::Template(source) = target else {
        return Err("只有模板目标可以框选图片".to_string());
    };
    let store = state.store.clone();
    blocking(move || {
        let png = store.read_asset(&source.asset).map_err(err)?;
        let area = (
            rect.x.round() as i32,
            rect.y.round() as i32,
            rect.w.round() as i32,
            rect.h.round() as i32,
        );
        let created = author::refine_template(&source, &png, area).map_err(err)?;
        store
            .put_asset_verified(&created.target.asset, &created.png)
            .map_err(err)?;
        Ok(Target::Template(created.target))
    })
    .await
}

#[tauri::command]
pub fn asset_png(state: State<'_, AppState>, sha: String) -> CmdResult<Response> {
    state.store.read_asset(&sha).map(Response::new).map_err(err)
}

// --------------------------------------------------------------------- runs

#[tauri::command]
pub async fn probe(
    state: State<'_, AppState>,
    ir: IrDoc,
    surface: Surface,
) -> CmdResult<Vec<ProbeRow>> {
    let store = state.store.clone();
    blocking(move || {
        let mut driver = platform::make_driver(surface);
        liverguard_engine::probe(&ir, &mut *driver, AssetStore::with_dir(store.assets_dir()))
    })
    .await
}

#[tauri::command]
pub async fn run_start(
    state: State<'_, AppState>,
    ir: IrDoc,
    surface: Surface,
    repeat: u32,
    on_message: Channel<Value>,
) -> CmdResult<String> {
    state.stop_live();
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut slot = state.run_slot.lock().unwrap();
        if slot.is_some() {
            return Err("已有脚本正在运行".to_string());
        }
        *slot = Some(stop.clone());
    }

    let store = state.store.clone();
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let id = run_id.clone();
    let release = state.run_slot.clone();
    std::thread::Builder::new()
        .name("liverguard-run".into())
        .spawn(move || {
            let done = execute(&store, &ir, surface, repeat, &id, stop, &on_message);
            let _ = on_message.send(done);
            *release.lock().unwrap() = None;
        })
        .map_err(err)?;
    Ok(run_id)
}

fn execute(
    store: &LocalStore,
    ir: &IrDoc,
    surface: Surface,
    repeat: u32,
    run_id: &str,
    stop: Arc<AtomicBool>,
    channel: &Channel<Value>,
) -> Value {
    let mut driver = platform::make_driver(surface);
    let events = channel.clone();
    let sink: EventSink = Arc::new(move |event| {
        if let Ok(value) = serde_json::to_value(event) {
            let _ = events.send(value);
        }
    });

    // A settings file that cannot be read must not silently turn alerts off.
    let alert = match store.load_settings() {
        Ok(settings) => Some(settings.notify).filter(|n| n.is_enabled()),
        Err(error) => {
            sink(&liverguard_engine::RunEvent::Log {
                level: "warn".into(),
                event: "alert_unavailable".into(),
                payload: json!({ "message": format!("读取通知设置失败，本次运行不会发送失败通知：{error}") }),
            });
            None
        }
    };

    let failed = |message: String| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        json!({ "kind": "done", "runId": run_id, "result": {
            "state": "failed", "stepCount": 0, "errorCode": "prepare", "errorMsg": message,
            "errorNode": null, "startedAt": now, "endedAt": now,
        }})
    };

    let assets = AssetStore::with_dir(store.assets_dir());
    match liverguard_engine::run_local(
        ir,
        &mut *driver,
        assets,
        run_id,
        stop,
        liverguard_engine::RunOptions {
            sink: Some(sink),
            rounds: repeat,
            alert,
        },
    ) {
        Err(message) => failed(message),
        Ok((result, reporter)) => {
            let mut record = result.to_json();
            record["summary"] = reporter.summary();
            record["script"] = Value::String(ir.meta.name.clone());
            let saved = store
                .save_run(record, &reporter.jsonl())
                .unwrap_or_else(|_| run_id.to_string());
            json!({ "kind": "done", "runId": saved, "result": result.to_json() })
        }
    }
}

#[tauri::command]
pub fn run_stop(state: State<'_, AppState>) {
    if let Some(stop) = state.run_slot.lock().unwrap().as_ref() {
        stop.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub fn list_runs(state: State<'_, AppState>) -> CmdResult<Vec<Value>> {
    state.store.list_runs().map_err(err)
}

#[tauri::command]
pub fn get_run(state: State<'_, AppState>, id: String) -> CmdResult<Value> {
    state.store.get_run(&id).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::export_file_name;

    #[test]
    fn export_file_names_are_safe_on_both_platforms() {
        assert_eq!(export_file_name("契灵"), "契灵.liverguard.json");
        assert_eq!(
            export_file_name(r#"a/b\c:d*e?f"g<h>i|j"#),
            "a_b_c_d_e_f_g_h_i_j.liverguard.json"
        );
        assert_eq!(export_file_name("  ..  "), "script.liverguard.json");
        assert_eq!(export_file_name(""), "script.liverguard.json");
    }
}
