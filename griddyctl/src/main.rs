//! griddyctl — GriddyWM control CLI
//!
//! Communicates with the compositor via the UNIX IPC sockets at
//! $XDG_RUNTIME_DIR/griddy/$GRIDDY_INSTANCE_SIGNATURE/.command.sock
//!
//! Phase 3 will implement the full socket protocol. For now this
//! provides the CLI skeleton and a simple dispatch command.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
};

/// GriddyWM control CLI
#[derive(Parser, Debug)]
#[command(name = "griddyctl", version, about)]
struct Args {
    /// Output JSON instead of human-readable text
    #[arg(short = 'j', long)]
    json: bool,

    /// GriddyWM instance signature (defaults to $GRIDDY_INSTANCE_SIGNATURE)
    #[arg(long)]
    instance: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Send a dispatcher command to the compositor
    Dispatch {
        /// Action name (e.g. workspace-left, close-window)
        action: String,
        /// Optional arguments
        args: Vec<String>,
    },
    /// Query compositor info
    #[command(subcommand)]
    Get(GetCommand),
    /// Reload configuration (optionally a specific part: theme, all)
    Reload {
        /// What to reload: "theme" or "all" (default)
        #[arg(default_value = "all")]
        target: String,
    },
    /// Print compositor version
    Version,
    /// Run multiple commands in one connection
    Batch {
        /// Semicolon-separated commands
        commands: String,
    },
    /// Shader subcommands
    #[command(subcommand)]
    Shader(ShaderCommand),
    /// Tail the event socket (line-delimited push events)
    Subscribe,
    /// Session persistence commands
    #[command(subcommand)]
    Session(SessionCommand),
    /// Runtime debug info and toggles (§22.14)
    Debug {
        /// Subcommand: surfaces, fps, safe_mode
        subcommand: Option<String>,
    },
    /// Import keybinds/decoration/animations from another compositor's config
    Import {
        /// Source compositor: hyprland, niri, or sway
        source: String,
        /// Path to the source config file
        config_path: PathBuf,
        /// Write output to this file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Print all currently-bound keys sorted by modifier+key (§22.19)
    Cheatsheet,
    /// Exit safe mode (re-enable user config without restarting) (§22.2)
    ExitSafeMode,
    /// Set a runtime-overridable config key
    Keyword {
        /// Config key (e.g. input.follow_mouse, gaps.windows.inner_px)
        key: String,
        /// New value
        value: String,
    },
    /// Get the current value of a config option
    Getoption {
        /// Config key (e.g. grid.cols, theme.osd.enabled)
        key: String,
    },
    /// Show OSD with a custom message
    Notify {
        /// Message / OSD kind to display
        message: String,
    },
    /// Grid management subcommands
    #[command(subcommand)]
    Grid(GridCommand),
    /// Workspace management subcommands
    #[command(subcommand)]
    Workspace(WorkspaceCommand),
    /// Plugin management subcommands (§13)
    #[command(subcommand)]
    Plugin(PluginCommand),
    /// List connected monitors (shorthand for `get monitors`)
    Monitors,
    /// Set a per-window property at runtime
    Setprop {
        /// Window ID (as shown by `get windows`)
        id: u64,
        /// Property name (is_urgent, sticky, pin, opacity, border_color, shader, no_animations, steal_focus, above_total_fullscreen)
        prop: String,
        /// New value
        value: String,
    },
}

#[derive(Subcommand, Debug)]
enum WorkspaceCommand {
    /// Apply the workspace template for a cell (§8.10)
    ApplyTemplate {
        /// Column (0-based)
        col: u8,
        /// Row (0-based)
        row: u8,
    },
    /// Rename a workspace cell
    Rename {
        /// Column (0-based)
        col: u8,
        /// Row (0-based)
        row: u8,
        /// New name (empty string to clear)
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum GridCommand {
    /// Resize the workspace grid (cols × rows, max 16×16)
    Resize {
        /// Number of columns (1–16)
        cols: u8,
        /// Number of rows (1–16)
        rows: u8,
    },
}

#[derive(Subcommand, Debug)]
enum SessionCommand {
    /// Save current window assignments to session file
    Save,
    /// Restore session: reload hints from session file for upcoming windows
    Restore,
    /// Clear saved session state
    Clear,
    /// Show session status (pending restores, safe mode flag)
    Status,
}

#[derive(Subcommand, Debug)]
enum PluginCommand {
    /// List loaded plugins
    List,
    /// Load a plugin from a .so path
    Load {
        /// Path to the plugin shared library
        path: String,
    },
    /// Unload a plugin by name
    Unload {
        /// Plugin name (as returned by `plugin list`)
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum GetCommand {
    /// Active workspace info
    Workspace,
    /// All workspaces
    Workspaces,
    /// All windows
    Windows,
    /// Active window
    Window,
    /// Grid dimensions
    Grid,
    /// Monitor list
    Monitors,
    /// Layer-shell surfaces
    Layers,
    /// Active shader info
    Shaders,
    /// Cursor position
    Cursorpos,
}

#[derive(Subcommand, Debug)]
enum ShaderCommand {
    /// Set per-window shader
    Set {
        #[arg(long)]
        window: Option<u64>,
        path: PathBuf,
    },
    /// Set screen post-process shader
    Screen { path: PathBuf },
    /// Clear screen shader
    Clear,
}

fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter("griddyctl=warn")
        .init();

    // Subscribe connects to the events socket, not the command socket.
    if matches!(args.command, Command::Subscribe) {
        let evt_path = resolve_events_socket_path(args.instance.as_deref())
            .context("Cannot find compositor events socket. Is GriddyWM running?")?;
        return subscribe(&evt_path);
    }

    let socket_path = resolve_socket_path(args.instance.as_deref())
        .context("Cannot find compositor socket. Is GriddyWM running?")?;

    match args.command {
        Command::Version => {
            let resp = send_command(&socket_path, "version", args.json)?;
            println!("{resp}");
        }
        Command::Reload { target } => {
            let cmd = if target == "all" || target.is_empty() {
                "reload".to_owned()
            } else {
                format!("reload {target}")
            };
            let resp = send_raw(&socket_path, &cmd, args.json)?;
            println!("{resp}");
        }
        Command::Dispatch { action, args: cmd_args } => {
            let cmd = if cmd_args.is_empty() {
                format!("dispatch {action}")
            } else {
                format!("dispatch {action} {}", cmd_args.join(" "))
            };
            let resp = send_raw(&socket_path, &cmd, args.json)?;
            println!("{resp}");
        }
        Command::Get(get_cmd) => {
            let query = match get_cmd {
                GetCommand::Workspace  => "activeworkspace",
                GetCommand::Workspaces => "workspaces",
                GetCommand::Windows    => "windows",
                GetCommand::Window     => "activewindow",
                GetCommand::Grid       => "grid",
                GetCommand::Monitors   => "monitors",
                GetCommand::Layers     => "layers",
                GetCommand::Shaders    => "shaders",
                GetCommand::Cursorpos  => "cursorpos",
            };
            let resp = send_command(&socket_path, query, args.json)?;
            println!("{resp}");
        }
        Command::Batch { commands } => {
            let cmd = format!("[[BATCH]] {commands}");
            let resp = send_raw(&socket_path, &cmd, false)?;
            println!("{resp}");
        }
        Command::Shader(shader_cmd) => {
            let cmd = match shader_cmd {
                ShaderCommand::Set { window, path } => {
                    if let Some(id) = window {
                        format!("dispatch set-window-shader {id} {}", path.display())
                    } else {
                        bail!("--window <id> required for shader set");
                    }
                }
                ShaderCommand::Screen { path } => {
                    format!("dispatch screen-shader {}", path.display())
                }
                ShaderCommand::Clear => "dispatch screen-shader clear".to_owned(),
            };
            let resp = send_raw(&socket_path, &cmd, false)?;
            println!("{resp}");
        }
        Command::Subscribe => unreachable!("handled above"),
        Command::Debug { subcommand } => {
            let sub = subcommand.as_deref().unwrap_or("");
            let resp = send_raw(&socket_path, &format!("debug {sub}"), args.json)?;
            println!("{resp}");
        }
        Command::Import { source, config_path, output } => {
            let toml_out = import_config(&source, &config_path)?;
            match output {
                Some(path) => {
                    std::fs::write(&path, &toml_out)
                        .with_context(|| format!("Failed to write {}", path.display()))?;
                    eprintln!("Written to {}", path.display());
                }
                None => println!("{toml_out}"),
            }
        }
        Command::Session(session_cmd) => {
            let sub = match session_cmd {
                SessionCommand::Save    => "save",
                SessionCommand::Restore => "restore",
                SessionCommand::Clear   => "clear",
                SessionCommand::Status  => "status",
            };
            let resp = send_raw(&socket_path, &format!("session {sub}"), args.json)?;
            println!("{resp}");
        }
        Command::Cheatsheet => {
            let resp = send_raw(&socket_path, "cheatsheet", args.json)?;
            println!("{resp}");
        }
        Command::ExitSafeMode => {
            let resp = send_raw(&socket_path, "exit-safe-mode", false)?;
            println!("{resp}");
        }
        Command::Grid(grid_cmd) => {
            let cmd = match grid_cmd {
                GridCommand::Resize { cols, rows } => format!("grid resize {cols} {rows}"),
            };
            let resp = send_raw(&socket_path, &cmd, args.json)?;
            println!("{resp}");
        }
        Command::Workspace(ws_cmd) => {
            let cmd = match ws_cmd {
                WorkspaceCommand::ApplyTemplate { col, row } =>
                    format!("dispatch workspace-apply-template {col},{row}"),
                WorkspaceCommand::Rename { col, row, name } =>
                    format!("dispatch workspace-rename {col},{row} {name}"),
            };
            let resp = send_raw(&socket_path, &cmd, args.json)?;
            println!("{resp}");
        }
        Command::Keyword { key, value } => {
            let resp = send_raw(&socket_path, &format!("keyword {key} {value}"), false)?;
            println!("{resp}");
        }
        Command::Getoption { key } => {
            let resp = send_raw(&socket_path, &format!("getoption {key}"), args.json)?;
            println!("{resp}");
        }
        Command::Notify { message } => {
            let resp = send_raw(&socket_path, &format!("notify {message}"), false)?;
            println!("{resp}");
        }
        Command::Plugin(plugin_cmd) => {
            let cmd = match plugin_cmd {
                PluginCommand::List             => "plugin list".to_owned(),
                PluginCommand::Load { path }    => format!("plugin load {path}"),
                PluginCommand::Unload { name }  => format!("plugin unload {name}"),
            };
            let resp = send_raw(&socket_path, &cmd, args.json)?;
            println!("{resp}");
        }
        Command::Monitors => {
            let resp = send_command(&socket_path, "monitors", args.json)?;
            println!("{resp}");
        }
        Command::Setprop { id, prop, value } => {
            let resp = send_raw(&socket_path, &format!("setprop {id} {prop} {value}"), false)?;
            println!("{resp}");
        }
    }

    Ok(())
}

fn resolve_socket_path(instance: Option<&str>) -> Option<PathBuf> {
    socket_path(instance, ".command.sock")
}

fn resolve_events_socket_path(instance: Option<&str>) -> Option<PathBuf> {
    socket_path(instance, ".events.sock")
}

fn socket_path(instance: Option<&str>, filename: &str) -> Option<PathBuf> {
    let sig = instance
        .map(|s| s.to_owned())
        .or_else(|| std::env::var("GRIDDY_INSTANCE_SIGNATURE").ok())?;

    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let path = PathBuf::from(runtime_dir)
        .join("griddy")
        .join(&sig)
        .join(filename);

    if path.exists() {
        Some(path)
    } else {
        tracing::debug!("Socket not found: {}", path.display());
        None
    }
}

fn subscribe(evt_path: &PathBuf) -> Result<()> {
    let stream =
        UnixStream::connect(evt_path).context("Failed to connect to events socket")?;
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        match line {
            Ok(l) => println!("{l}"),
            Err(e) => {
                tracing::debug!("Event stream ended: {e}");
                break;
            }
        }
    }
    Ok(())
}

fn send_command(socket_path: &PathBuf, command: &str, json: bool) -> Result<String> {
    let cmd = if json {
        format!("j/{command}")
    } else {
        command.to_owned()
    };
    send_raw(socket_path, &cmd, false)
}

// ─── Config import (§22.15) ──────────────────────────────────────────────────

fn import_config(source: &str, path: &PathBuf) -> Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read {}", path.display()))?;
    match source {
        "hyprland" => Ok(import_hyprland(&content)),
        "sway" | "i3" => Ok(import_sway(&content)),
        "niri" => Ok(import_niri(&content)),
        other => bail!("Unknown source compositor '{other}'. Use: hyprland, niri, sway"),
    }
}

/// Best-effort Hyprland config → GriddyWM TOML translator.
///
/// Translates:
///   - `bind = $MOD, key, action, ...` → `[[bind]]` entries
///   - `general { gaps_in, gaps_out, border_size, col.active_border, col.inactive_border }`
///   - `decoration { rounding, blur }`
///   - `animations { enabled, bezier, animation ... }` → `[animations]`
fn import_hyprland(src: &str) -> String {
    let mut binds: Vec<String> = Vec::new();
    let mut gaps_inner = 8i32;
    let mut gaps_outer = 12i32;
    let mut border_px = 2i32;
    let mut accent = "#7aa2f7".to_owned();
    let mut border_idle = "#414868".to_owned();
    let mut blur_enabled = true;
    let mut blur_passes = 2u32;
    let mut open_ms = 160u32;
    let mut unmapped: Vec<String> = Vec::new();

    for raw_line in src.lines() {
        let line = raw_line.trim();

        // ── binds ────────────────────────────────────────────────────────────
        if let Some(rest) = line.strip_prefix("bind") {
            // `bind = SUPER, Q, killactive`
            // `bind = SUPER SHIFT, Return, exec, kitty`
            let rest = rest.trim_start_matches([' ', '=']);
            let parts: Vec<&str> = rest.splitn(4, ',').map(|s| s.trim()).collect();
            if parts.len() >= 3 {
                let mods = parts[0].replace("SUPER", "$mod").replace("SHIFT", "Shift")
                    .replace("CTRL", "Ctrl").replace("ALT", "Alt");
                let key = parts[1];
                let action = parts[2];
                let extra = parts.get(3).copied().unwrap_or("");

                let griddy_action = hypr_action_to_griddy(action, extra);
                if !griddy_action.is_empty() {
                    let mods_toml = if mods.trim().is_empty() {
                        String::new()
                    } else {
                        format!("mods = [{:?}]\n", mods.trim())
                    };
                    binds.push(format!(
                        "[[bind]]\n{}key = {:?}\naction = {:?}",
                        mods_toml, key, griddy_action
                    ));
                } else {
                    unmapped.push(format!("# UNMAPPED: {line}"));
                }
            }
            continue;
        }

        // ── gaps / borders ───────────────────────────────────────────────────
        if let Some(v) = extract_kv(line, "gaps_in") {
            gaps_inner = v.trim().parse().unwrap_or(gaps_inner);
        }
        if let Some(v) = extract_kv(line, "gaps_out") {
            gaps_outer = v.trim().parse().unwrap_or(gaps_outer);
        }
        if let Some(v) = extract_kv(line, "border_size") {
            border_px = v.trim().parse().unwrap_or(border_px);
        }
        if let Some(v) = extract_kv(line, "col.active_border") {
            accent = parse_hypr_color(&v);
        }
        if let Some(v) = extract_kv(line, "col.inactive_border") {
            border_idle = parse_hypr_color(&v);
        }

        // ── blur ─────────────────────────────────────────────────────────────
        if let Some(v) = extract_kv(line, "blur:enabled") {
            blur_enabled = !matches!(v.trim(), "false" | "0" | "no");
        }
        if let Some(v) = extract_kv(line, "blur:passes") {
            blur_passes = v.trim().parse().unwrap_or(blur_passes);
        }

        // ── animations ───────────────────────────────────────────────────────
        if line.starts_with("animation = windows,") {
            // `animation = windows, 1, 7, default`
            let parts: Vec<&str> = line.splitn(5, ',').collect();
            if parts.len() >= 3 {
                if let Ok(speed) = parts[2].trim().parse::<f64>() {
                    open_ms = ((speed * 30.0) as u32).max(50);
                }
            }
        }
    }

    let mut out = String::new();
    out.push_str("# Imported from Hyprland config — review and adjust before use\n\n");

    out.push_str("[gaps.windows]\n");
    out.push_str(&format!("inner_px = {gaps_inner}\n"));
    out.push_str(&format!("outer_px = {gaps_outer}\n\n"));

    out.push_str("[colors]\n");
    out.push_str(&format!("accent = {:?}\n", accent));
    out.push_str(&format!("border_idle = {:?}\n\n", border_idle));

    out.push_str("[window.focused]\n");
    out.push_str(&format!("border_px = {border_px}\n\n"));

    out.push_str("[window.unfocused]\n");
    out.push_str(&format!("border_px = {border_px}\n\n"));

    out.push_str("[blur]\n");
    out.push_str(&format!("enabled = {blur_enabled}\n"));
    out.push_str(&format!("passes = {blur_passes}\n\n"));

    out.push_str("[animations]\n");
    out.push_str(&format!("open_duration_ms = {open_ms}\n\n"));

    for bind in &binds {
        out.push_str(bind);
        out.push('\n');
    }

    if !unmapped.is_empty() {
        out.push_str("\n# The following Hyprland binds could not be automatically translated:\n");
        for u in &unmapped {
            out.push_str(u);
            out.push('\n');
        }
    }

    out
}

fn hypr_action_to_griddy(action: &str, args: &str) -> String {
    match action {
        "killactive"          => "close-window".into(),
        "exit"                => "quit".into(),
        "exec"                => format!("exec {args}"),
        "togglefloating"      => "toggle-floating".into(),
        "fullscreen"          => if args == "1" { "toggle-fullscreen".into() } else { "toggle-total-fullscreen".into() }
        "pseudo"              => "toggle-floating".into(),
        "movefocus"           => {
            match args { "l" => "focus-left", "r" => "focus-right", "u" => "focus-up", "d" => "focus-down", _ => "" }.into()
        }
        "movewindow"          => {
            match args { "l" => "move-window-left", "r" => "move-window-right", "u" => "move-window-up", "d" => "move-window-down", _ => "" }.into()
        }
        "workspace"           => {
            if let Ok(n) = args.trim().parse::<u8>() {
                format!("workspace {}", n - 1)
            } else { String::new() }
        }
        "movetoworkspace"     => {
            if let Ok(n) = args.trim().parse::<u8>() {
                format!("move-window-to {}", n - 1)
            } else { String::new() }
        }
        "togglespecialworkspace" => format!("toggle-special {}", if args.is_empty() { "scratchpad" } else { args }),
        "movetoworkspacesilent" => {
            if let Ok(n) = args.trim().parse::<u8>() {
                format!("move-window-to {}", n - 1)
            } else { String::new() }
        }
        _ => String::new(),
    }
}

fn parse_hypr_color(s: &str) -> String {
    // Hyprland: `rgba(7aa2f7ff)` or `0xff7aa2f7` or `#7aa2f7`
    let s = s.trim().trim_matches('"');
    if s.starts_with("rgba(") {
        let inner = s.trim_start_matches("rgba(").trim_end_matches(')');
        if inner.len() == 8 {
            return format!("#{}", &inner[..6]);
        }
    }
    if s.starts_with("0xff") || s.starts_with("0xFF") {
        return format!("#{}", &s[4..]);
    }
    s.to_owned()
}

fn extract_kv<'a>(line: &'a str, key: &str) -> Option<String> {
    // Match `key = value` or `key=value`
    let stripped = line.trim();
    if stripped.starts_with(key) {
        let rest = stripped[key.len()..].trim_start();
        if rest.starts_with('=') {
            return Some(rest[1..].trim().to_owned());
        }
    }
    None
}

/// Best-effort Sway/i3 config → GriddyWM TOML translator.
fn import_sway(src: &str) -> String {
    let mut binds: Vec<String> = Vec::new();
    let mut gaps_inner = 8i32;
    let mut gaps_outer = 12i32;
    let mut border_px = 2i32;
    let mut unmapped: Vec<String> = Vec::new();

    for raw_line in src.lines() {
        let line = raw_line.trim();

        if let Some(rest) = line.strip_prefix("bindsym ") {
            // `bindsym $mod+q kill`
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let keys: Vec<&str> = parts[0].split('+').collect();
                let mods_vec: Vec<String> = keys[..keys.len().saturating_sub(1)]
                    .iter()
                    .map(|m| match *m {
                        "$mod" | "Mod4" => "$mod".into(),
                        "Shift"         => "Shift".into(),
                        "Ctrl" | "Control" => "Ctrl".into(),
                        "Alt" | "Mod1"  => "Alt".into(),
                        other           => other.into(),
                    })
                    .collect();
                let key = keys.last().copied().unwrap_or("");
                let action = sway_action_to_griddy(parts[1]);
                if !action.is_empty() {
                    let mods_toml = if mods_vec.is_empty() {
                        String::new()
                    } else {
                        let joined = mods_vec.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join(", ");
                        format!("mods = [{joined}]\n")
                    };
                    binds.push(format!("[[bind]]\n{}key = {:?}\naction = {:?}", mods_toml, key, action));
                } else {
                    unmapped.push(format!("# UNMAPPED: {line}"));
                }
            }
            continue;
        }

        if line.starts_with("gaps inner") {
            let v: i32 = line.split_whitespace().last().unwrap_or("").parse().unwrap_or(gaps_inner);
            gaps_inner = v;
        }
        if line.starts_with("gaps outer") {
            let v: i32 = line.split_whitespace().last().unwrap_or("").parse().unwrap_or(gaps_outer);
            gaps_outer = v;
        }
        if line.starts_with("default_border pixel") {
            let v: i32 = line.split_whitespace().last().unwrap_or("").parse().unwrap_or(border_px);
            border_px = v;
        }
    }

    let mut out = String::from("# Imported from Sway/i3 config — review and adjust before use\n\n");
    out.push_str("[gaps.windows]\n");
    out.push_str(&format!("inner_px = {gaps_inner}\nouter_px = {gaps_outer}\n\n"));
    out.push_str("[window.focused]\n");
    out.push_str(&format!("border_px = {border_px}\n\n"));

    for bind in &binds { out.push_str(bind); out.push('\n'); }
    if !unmapped.is_empty() {
        out.push_str("\n# Unmapped Sway binds:\n");
        for u in &unmapped { out.push_str(u); out.push('\n'); }
    }
    out
}

fn sway_action_to_griddy(action: &str) -> String {
    let action = action.trim();
    match action {
        "kill"       => "close-window".into(),
        "exit"       => "quit".into(),
        "floating toggle" => "toggle-floating".into(),
        "fullscreen toggle" => "toggle-fullscreen".into(),
        a if a.starts_with("exec ") => a.to_owned(),
        a if a.starts_with("focus ") => {
            match a.strip_prefix("focus ").unwrap_or("").trim() {
                "left"  => "focus-left".into(),
                "right" => "focus-right".into(),
                "up"    => "focus-up".into(),
                "down"  => "focus-down".into(),
                _ => String::new(),
            }
        }
        a if a.starts_with("move ") => {
            let rest = a.strip_prefix("move ").unwrap_or("").trim();
            match rest {
                "left"  => "move-window-left".into(),
                "right" => "move-window-right".into(),
                "up"    => "move-window-up".into(),
                "down"  => "move-window-down".into(),
                _ => String::new(),
            }
        }
        a if a.starts_with("workspace ") => {
            let ws = a.strip_prefix("workspace ").unwrap_or("").trim();
            if let Ok(n) = ws.parse::<u8>() {
                format!("workspace {}", n - 1)
            } else { String::new() }
        }
        _ => String::new(),
    }
}

/// Best-effort Niri config (KDL) → GriddyWM TOML translator.
fn import_niri(src: &str) -> String {
    let mut binds: Vec<String> = Vec::new();
    let mut gaps = 16i32;
    let mut border_px = 2i32;
    let mut unmapped: Vec<String> = Vec::new();

    for raw_line in src.lines() {
        let line = raw_line.trim();

        // Niri keybind: `Mod+q { close-window; }`
        if line.contains('{') {
            let before = line.split('{').next().unwrap_or("").trim();
            let action_part = line.split('{').nth(1).unwrap_or("").trim().trim_end_matches('}').trim();
            let keys: Vec<&str> = before.split('+').collect();
            if keys.len() >= 2 {
                let mods_vec: Vec<String> = keys[..keys.len()-1].iter().map(|m| match *m {
                    "Mod" | "Super" => "$mod".into(),
                    "Shift"         => "Shift".into(),
                    "Ctrl"          => "Ctrl".into(),
                    "Alt"           => "Alt".into(),
                    other           => other.into(),
                }).collect();
                let key = keys.last().copied().unwrap_or("").trim_end_matches(';');
                let action = niri_action_to_griddy(action_part.trim_end_matches(';').trim());
                if !action.is_empty() {
                    let mods_toml = if mods_vec.is_empty() { String::new() }
                    else {
                        let joined = mods_vec.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join(", ");
                        format!("mods = [{joined}]\n")
                    };
                    binds.push(format!("[[bind]]\n{}key = {:?}\naction = {:?}", mods_toml, key, action));
                } else {
                    unmapped.push(format!("# UNMAPPED: {line}"));
                }
            }
        }

        if line.starts_with("gaps") {
            let v: i32 = line.split_whitespace().last().unwrap_or("").trim_end_matches(';').parse().unwrap_or(gaps);
            gaps = v;
        }
        if line.starts_with("border { width") || line.starts_with("border-width") {
            let v: i32 = line.split_whitespace().last().unwrap_or("").trim_end_matches(';').parse().unwrap_or(border_px);
            border_px = v;
        }
    }

    let mut out = String::from("# Imported from Niri config — review and adjust before use\n\n");
    out.push_str("[gaps.windows]\n");
    out.push_str(&format!("inner_px = {gaps}\nouter_px = {gaps}\n\n"));
    out.push_str("[window.focused]\n");
    out.push_str(&format!("border_px = {border_px}\n\n"));

    for bind in &binds { out.push_str(bind); out.push('\n'); }
    if !unmapped.is_empty() {
        out.push_str("\n# Unmapped Niri binds:\n");
        for u in &unmapped { out.push_str(u); out.push('\n'); }
    }
    out
}

fn niri_action_to_griddy(action: &str) -> String {
    match action {
        "close-window"           => "close-window".into(),
        "quit"                   => "quit".into(),
        "toggle-fullscreen"      => "toggle-fullscreen".into(),
        "toggle-window-floating" => "toggle-floating".into(),
        "focus-column-left"      => "focus-left".into(),
        "focus-column-right"     => "focus-right".into(),
        "focus-window-up"        => "focus-up".into(),
        "focus-window-down"      => "focus-down".into(),
        a if a.starts_with("spawn") => {
            let cmd = a.strip_prefix("spawn").unwrap_or("").trim().trim_matches('"');
            format!("exec {cmd}")
        }
        _ => String::new(),
    }
}

fn send_raw(socket_path: &PathBuf, command: &str, _json: bool) -> Result<String> {
    let mut stream =
        UnixStream::connect(socket_path).context("Failed to connect to compositor socket")?;
    stream
        .write_all(format!("{command}\n").as_bytes())
        .context("Failed to send command")?;
    stream.flush()?;

    // Read response until EOF
    let mut response = String::new();
    let mut reader = BufReader::new(&stream);
    reader
        .read_line(&mut response)
        .context("Failed to read response")?;

    Ok(response.trim().to_owned())
}
