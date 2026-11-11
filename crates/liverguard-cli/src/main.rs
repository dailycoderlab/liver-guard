//! `liverguard`: the command line front end of the LiverGuard engine.

mod platform;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use liverguard_engine::{EventSink, RunEvent, RunState};
use liverguard_ir::IrDoc;
use liverguard_store::LocalStore;
use liverguard_vision::AssetStore;

#[derive(Parser)]
#[command(name = "liverguard", version, about = "LiverGuard local script runner")]
struct Cli {
    /// Data directory (default ~/.liverguard)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check permissions, capture and coordinates
    Doctor {
        /// Check a window instead of the whole display (name, or #number)
        #[arg(long)]
        window: Option<String>,
    },
    /// List controllable windows
    Windows {
        /// Include off-screen and non-zero layers
        #[arg(long)]
        all: bool,
    },
    /// Save one screenshot (also verifies the capture permission)
    Capture {
        #[arg(long, default_value = "capture.png")]
        out: PathBuf,
        #[arg(long)]
        window: Option<String>,
    },
    /// Run a script
    Run(RunArgs),
    /// Dry run: locate every target once, click nothing
    Probe(RunArgs),
    /// List scripts in the local store
    Scripts,
    /// Export a script and its template images to one file
    Export {
        /// Script id or exact name in the local store
        #[arg(long)]
        script: String,
        /// File to write (default: <name>.liverguard.json in the current directory)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Send a test message through the configured failure webhook
    NotifyTest,
    /// Import an exported script file as a new script
    Import {
        /// File written by `export` or by the desktop app
        file: PathBuf,
    },
    /// Print the JSON Schema of the script document
    Schema,
}

#[derive(Args)]
struct RunArgs {
    /// Script id or exact name in the local store
    #[arg(long, conflicts_with = "file", required_unless_present = "file")]
    script: Option<String>,
    /// Script document (IR v2 JSON)
    #[arg(long)]
    file: Option<PathBuf>,
    /// Target window (name, or #number); default is the whole display
    #[arg(long)]
    window: Option<String>,
    /// Walk the script this many times in one run (default 1); a failure or Ctrl-C ends all rounds
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    repeat: u32,
    /// Do not send the failure webhook even if one is configured
    #[arg(long)]
    no_notify: bool,
    /// Safety limit for a local run
    #[arg(long)]
    max_seconds: Option<u64>,
    /// Write the run log (JSON Lines) here
    #[arg(long)]
    log: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(1)
        }
    }
}

fn dispatch(cli: Cli) -> Result<ExitCode> {
    let store = match &cli.root {
        Some(root) => LocalStore::new(root),
        None => LocalStore::open_default(),
    };
    match cli.command {
        Command::Doctor { window } => doctor(window.as_deref()),
        Command::Windows { all } => {
            for row in platform::list_windows(all) {
                println!("{}", row.label);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Capture { out, window } => capture(&out, window.as_deref()),
        Command::Run(args) => run(&store, args, false),
        Command::Probe(args) => run(&store, args, true),
        Command::Export { script, out } => export(&store, &script, out),
        Command::NotifyTest => {
            let config = store.load_settings()?.notify;
            liverguard_notify::send(&config, &liverguard_notify::test_message())
                .map_err(|e| anyhow!("{e}"))?;
            println!("测试通知已发送（接收方：{:?}）", config.channel);
            Ok(ExitCode::SUCCESS)
        }
        Command::Import { file } => import(&store, &file),
        Command::Scripts => {
            for script in store.list_scripts()? {
                println!("{}  {}", script.id, script.name);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Schema => {
            println!(
                "{}",
                serde_json::to_string_pretty(&liverguard_ir::json_schema())?
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn doctor(window: Option<&str>) -> Result<ExitCode> {
    let permissions_ok = platform::doctor_report();
    let mut driver = platform::make_driver(window)?;
    let started = Instant::now();
    match driver.capture() {
        Ok(frame) => {
            let bounds = frame.bounds;
            println!(
                "截图成功：{}x{} 像素，表面 {:.0}x{:.0}，缩放 {:.2}，耗时 {} ms",
                frame.width(),
                frame.height(),
                bounds.w,
                bounds.h,
                frame.scale(),
                started.elapsed().as_millis()
            );
            let std = frame.content_std().unwrap_or(0.0);
            if std < 2.0 {
                println!("警告：画面几乎是纯色（标准差 {std:.2}），可能是权限缺失或目标被遮住");
                return Ok(ExitCode::from(1));
            }
        }
        Err(error) => {
            println!("截图失败：{error}");
            return Ok(ExitCode::from(1));
        }
    }
    Ok(if permissions_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn capture(out: &PathBuf, window: Option<&str>) -> Result<ExitCode> {
    let mut driver = platform::make_driver(window)?;
    let frame = driver.capture().map_err(|e| anyhow!("{e}"))?;
    std::fs::write(out, frame.to_png().map_err(|e| anyhow!("{e}"))?)?;
    println!(
        "saved {} ({}x{} px)",
        out.display(),
        frame.width(),
        frame.height()
    );
    Ok(ExitCode::SUCCESS)
}

/// Resolve a script id or exact name to its id.
fn script_id(store: &LocalStore, wanted: &str) -> Result<String> {
    if store.get_script(wanted).is_ok() {
        return Ok(wanted.to_string());
    }
    let matches: Vec<_> = store
        .list_scripts()?
        .into_iter()
        .filter(|s| s.name == wanted)
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => bail!(
            "no script with id or name \"{wanted}\" in {}",
            store.root().display()
        ),
        _ => bail!("several scripts are named \"{wanted}\"; use the id"),
    }
}

fn export(store: &LocalStore, wanted: &str, out: Option<PathBuf>) -> Result<ExitCode> {
    let bundle = store.export_script(&script_id(store, wanted)?)?;
    let path = out.unwrap_or_else(|| {
        let safe: String = bundle
            .name
            .chars()
            .map(|c| {
                if c.is_control() || "/\\:*?\"<>|".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        PathBuf::from(format!("{}.liverguard.json", safe.trim().trim_matches('.')))
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&bundle)?)
        .with_context(|| format!("writing {}", path.display()))?;
    println!(
        "exported \"{}\" ({} images) to {}",
        bundle.name,
        bundle.assets.len(),
        path.display()
    );
    Ok(ExitCode::SUCCESS)
}

fn import(store: &LocalStore, file: &PathBuf) -> Result<ExitCode> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let record = store.import_bytes(&bytes)?;
    println!("{}  {}", record.id, record.name);
    Ok(ExitCode::SUCCESS)
}

fn load_script(store: &LocalStore, args: &RunArgs) -> Result<IrDoc> {
    if let Some(path) = &args.file {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        return serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()));
    }
    let wanted = args.script.as_deref().unwrap_or_default();
    if let Ok(record) = store.get_script(wanted) {
        return Ok(record.ir);
    }
    let matches: Vec<_> = store
        .list_scripts()?
        .into_iter()
        .filter(|s| s.name == wanted)
        .collect();
    match matches.as_slice() {
        [one] => Ok(store.get_script(&one.id)?.ir),
        [] => bail!(
            "no script with id or name \"{wanted}\" in {}",
            store.root().display()
        ),
        _ => bail!("several scripts are named \"{wanted}\"; use the id"),
    }
}

fn print_event(event: &RunEvent) {
    if let RunEvent::Log {
        level,
        event,
        payload,
    } = event
    {
        if level == "debug" {
            return;
        }
        let payload = if payload.is_null() {
            String::new()
        } else {
            payload.to_string()
        };
        eprintln!("[{level}] {event} {payload}");
    }
}

fn run(store: &LocalStore, args: RunArgs, dry: bool) -> Result<ExitCode> {
    let ir = load_script(store, &args)?;
    let assets = AssetStore::with_dir(store.assets_dir());
    let mut driver = platform::make_driver(args.window.as_deref())?;

    if dry {
        let rows = liverguard_engine::probe(&ir, &mut *driver, assets).map_err(|e| anyhow!(e))?;
        println!("{}", serde_json::to_string_pretty(&rows)?);
        let all_ok = rows.iter().all(|row| row.ok);
        return Ok(if all_ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }

    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))
        .context("installing Ctrl-C handler")?;
    if let Some(seconds) = args.max_seconds {
        let flag = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(seconds));
            flag.store(true, Ordering::Relaxed);
        });
    }

    // Failure alerts come from the settings the desktop app saved (same store, same file).
    let alert = if args.no_notify {
        None
    } else {
        Some(store.load_settings()?.notify).filter(|n| n.is_enabled())
    };
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let sink: EventSink = Arc::new(print_event);
    let (result, reporter) = liverguard_engine::run_local(
        &ir,
        &mut *driver,
        assets,
        &run_id,
        stop,
        liverguard_engine::RunOptions {
            sink: Some(sink),
            rounds: args.repeat,
            alert,
        },
    )
    .map_err(|e| anyhow!(e))?;

    let jsonl = reporter.jsonl();
    if let Some(path) = &args.log {
        std::fs::write(path, &jsonl).with_context(|| format!("writing {}", path.display()))?;
    }
    let mut record = result.to_json();
    record["summary"] = reporter.summary();
    record["script"] = serde_json::Value::String(ir.meta.name.clone());
    store.save_run(record.clone(), &jsonl)?;
    println!("{}", serde_json::to_string_pretty(&record)?);

    Ok(match result.state {
        RunState::Done => ExitCode::SUCCESS,
        RunState::Failed => ExitCode::from(1),
        RunState::Stopped => ExitCode::from(130),
    })
}
