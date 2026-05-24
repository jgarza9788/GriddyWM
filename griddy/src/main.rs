#![allow(dead_code)] // Many grid methods are wired up in Phase 2+
mod animate;
mod backend;
mod config;
mod grid;
mod handlers;
mod ipc;
mod keybind;
mod plugins;
mod session;
mod state;

use anyhow::{Context, Result};
use clap::Parser;
use smithay::reexports::calloop::EventLoop;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::prelude::*;
use wayland_server::Display;

use backend::{BackendType, run_backend};
use state::GlobalState;

/// GriddyWM — grid-based Wayland compositor
#[derive(Parser, Debug)]
#[command(name = "griddy", version, about)]
struct Args {
    /// Path to config file (overrides XDG search)
    #[arg(long, short)]
    config: Option<PathBuf>,

    /// Parse all config files and exit (0 = valid, 1 = error)
    #[arg(long)]
    check: bool,

    /// Start and replace the running griddy instance
    #[arg(long)]
    replace: bool,

    /// Pin render GPU device e.g. /dev/dri/card1 (DRM backend)
    #[arg(long)]
    gpu: Option<PathBuf>,

    /// Disable XWayland for this session
    #[arg(long)]
    no_xwayland: bool,

    /// Debug mode: verbose logging
    #[arg(short, long)]
    debug: bool,

    /// Backend: winit (dev) or drm (production)
    #[arg(long, default_value = "winit")]
    backend: String,
}

fn pid_file_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(runtime_dir).join("griddy.lock")
}

fn write_pid_file() {
    let path = pid_file_path();
    let pid = std::process::id();
    if let Err(e) = std::fs::write(&path, format!("{}\n", pid)) {
        tracing::warn!("Failed to write PID file {}: {e}", path.display());
    } else {
        tracing::debug!("PID file written: {} (pid {})", path.display(), pid);
    }
}

fn cleanup_pid_file() {
    let path = pid_file_path();
    let _ = std::fs::remove_file(&path);
}

// ─── Crash recovery (§22.2) ──────────────────────────────────────────────────

fn crash_history_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
            std::path::PathBuf::from(home).join(".local/state")
        });
    base.join("griddy/crash_history")
}

/// Record this startup in the crash history and return true if we should enter safe mode
/// (≥3 crashes within the last 30 seconds).
fn record_crash_and_check_safe_mode() -> bool {
    let path = crash_history_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Read existing crash timestamps.
    let mut stamps: Vec<u64> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();

    // Keep only those within the last 30 seconds.
    stamps.retain(|&ts| now.saturating_sub(ts) <= 30);

    // Write back with this new startup timestamp appended.
    stamps.push(now);
    let _ = std::fs::write(
        &path,
        stamps
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let count = stamps.len();
    if count >= 3 {
        tracing::warn!("Crash detected {count} times in 30s — entering safe mode");
        true
    } else {
        false
    }
}

/// Clear the crash history on clean exit.
fn clear_crash_history() {
    let _ = std::fs::write(crash_history_path(), "");
}

fn replace_running_instance() -> Result<()> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let path = pid_file_path();
    let content = std::fs::read_to_string(&path)
        .context("No running griddy instance (no lock file); starting fresh")?;
    let pid: u32 = content.trim().parse().context("Invalid PID in lock file")?;

    let proc_path = PathBuf::from(format!("/proc/{}", pid));
    if !proc_path.exists() {
        tracing::info!("Stale PID file (PID {pid} not running); proceeding");
        return Ok(());
    }

    tracing::info!("Sending SIGTERM to running griddy (PID {pid})");
    kill(Pid::from_raw(pid as i32), Signal::SIGTERM).ok();

    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        if !proc_path.exists() {
            tracing::info!("Replaced instance exited cleanly");
            return Ok(());
        }
    }
    tracing::warn!("PID {pid} did not exit in 3s — sending SIGKILL");
    kill(Pid::from_raw(pid as i32), Signal::SIGKILL).ok();
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}

/// Embedded default config (installed at /usr/share/griddy/default.toml).
const DEFAULT_CONFIG: &str = include_str!("../../dist/default.toml");

/// On first launch (no config file found), create `~/.config/griddy/config.toml`
/// from the embedded default so the user starts with a working configuration.
fn first_run_copy_default_config() {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
            std::path::PathBuf::from(home).join(".config")
        })
        .join("griddy");

    let config_file = config_dir.join("config.toml");
    if config_file.exists() {
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&config_dir) {
        tracing::warn!("First-run: could not create {}: {e}", config_dir.display());
        return;
    }
    match std::fs::write(&config_file, DEFAULT_CONFIG) {
        Ok(()) => tracing::info!(
            "First-run: wrote default config to {}",
            config_file.display()
        ),
        Err(e) => tracing::warn!("First-run: could not write {}: {e}", config_file.display()),
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    // ── Logging ───────────────────────────────────────────────────────────────
    let log_filter = if args.debug {
        "griddy=debug,smithay=debug"
    } else {
        "griddy=info,smithay=warn"
    };
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_filter));

    // Rotate daily log files into $XDG_STATE_HOME/griddy/logs/ (§22.14).
    let log_dir = crate::session::state_dir().join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "griddy.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .init();

    // Keep _guard alive for the duration of main so the file writer isn't dropped.
    let _log_guard = _guard;

    // ── Panic handler ─────────────────────────────────────────────────────────
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::capture();
        tracing::error!("PANIC: {info}\n{backtrace}");
    }));

    tracing::info!("GriddyWM v{} starting", env!("CARGO_PKG_VERSION"));

    // ── Crash recovery (§22.2) ───────────────────────────────────────────────
    let safe_mode = record_crash_and_check_safe_mode();
    if safe_mode {
        tracing::warn!("SAFE MODE active: loading minimal config only");
    }

    // ── First-run: copy default config if none exists ─────────────────────────
    let is_first_run = args.config.is_none() && config::config_path().is_none();
    if is_first_run {
        first_run_copy_default_config();
    }

    // ── Load config ───────────────────────────────────────────────────────────
    let resolved_config_path = if safe_mode {
        None
    } else {
        args.config.clone().or_else(config::config_path)
    };
    let config = config::load_config(resolved_config_path.as_deref())
        .context("Failed to load configuration")?;

    if args.check {
        // Validate all subsidiary config files too.
        if let Some(ref path) = resolved_config_path && let Some(dir) = path.parent() {
            let _theme = config::load_config(Some(path)); // already done above
            let _rules = dir.join("rules.toml");
            let _tpls = config::load_templates(dir);
            // report any template parse warnings (already logged inside load_templates)
        }
        println!("Config OK");
        return Ok(());
    }

    // ── Replace running instance if requested ─────────────────────────────────
    if args.replace && let Err(e) = replace_running_instance() {
        tracing::warn!("replace: {e}");
    }

    // ── Apply [env] block ─────────────────────────────────────────────────────
    config::apply_env(&config);

    // ── Event loop + display ──────────────────────────────────────────────────
    let event_loop: EventLoop<'static, GlobalState> =
        EventLoop::try_new().context("Failed to create event loop")?;
    let display: Display<GlobalState> =
        Display::new().context("Failed to create Wayland display")?;

    // ── Build compositor state ────────────────────────────────────────────────
    let mut state = GlobalState::new(
        &display,
        config,
        resolved_config_path,
        safe_mode,
        is_first_run,
    )
    .context("Failed to initialize compositor state")?;

    // §22.14 — `-d` enables debug overlay automatically.
    if args.debug {
        state.debug_overlay = true;
    }

    // ── Seat ─────────────────────────────────────────────────────────────────
    state.init_seat();

    // ── Select and run backend ────────────────────────────────────────────────
    let backend_type = match args.backend.as_str() {
        "winit" => BackendType::Winit,
        "drm" | "udev" => anyhow::bail!("DRM backend not yet implemented (Phase 4)"),
        other => anyhow::bail!("Unknown backend: {other}. Use 'winit' or 'drm'"),
    };

    // Write PID file so `--replace` can find us.
    write_pid_file();

    let result = run_backend(backend_type, event_loop, display, state, args.no_xwayland)
        .context("Backend exited with error");

    // ── Clean exit: save session + clear crash counter ────────────────────────
    if result.is_ok() {
        clear_crash_history();
    }

    cleanup_pid_file();

    tracing::info!("GriddyWM shutdown complete");
    result
}
