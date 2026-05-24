//! IPC command handler — maps text commands to compositor actions and queries (§12.1).

use crate::grid::window::{Slot, WindowState};
use crate::keybind::dispatcher;
use crate::state::GlobalState;

// ─── Public entry point ───────────────────────────────────────────────────────

/// Commands that mutate compositor state or spawn processes; blocked while locked.
fn is_locked_blocked(verb: &str) -> bool {
    matches!(
        verb,
        "kill" | "dispatch" | "reload" | "keyword" | "setprop" | "notify" | "plugin"
    )
}

/// Handle a single command line, return the response string.
pub fn handle(cmd: &str, state: &mut GlobalState) -> String {
    if let Some(rest) = cmd.strip_prefix("[[BATCH]]") {
        return handle_batch(rest.trim(), state);
    }

    let (json, inner) = if let Some(rest) = cmd.strip_prefix("j/") {
        (true, rest)
    } else {
        (false, cmd)
    };

    let (verb, rest) = split_verb(inner);

    if state.is_locked && is_locked_blocked(verb) {
        tracing::debug!("IPC command '{verb}' blocked: session is locked");
        return if json {
            r#"{"ok":false,"error":"session is locked"}"#.into()
        } else {
            "error: session is locked".into()
        };
    }

    match verb {
        "version" => version(json),
        "reload" => reload_cmd(rest, state),
        "kill" => {
            state.should_exit = true;
            "ok".into()
        }
        "dispatch" => dispatch_cmd(rest, json, state),
        "workspaces" => workspaces(json, state),
        "windows" => windows(json, state),
        "activewindow" => active_window(json, state),
        "activeworkspace" => active_workspace(json, state),
        "grid" => grid_cmd(rest, json, state),
        "monitors" => monitors(json, state),
        "cursorpos" => {
            let (x, y) = state.cursor_pos;
            let xi = x as i64;
            let yi = y as i64;
            if json {
                format!(r#"{{"x":{xi},"y":{yi}}}"#)
            } else {
                format!("{xi},{yi}")
            }
        }
        "keyword" => keyword_cmd(rest, json, state),
        "layers" => layers_cmd(json, state),
        "animations" => animations_info(json, state),
        "shaders" => shaders_cmd(json, state),
        "getoption" => getoption_cmd(rest, json, state),
        "setprop" => setprop_cmd(rest, state),
        "notify" => notify_cmd(rest, state),
        "globalshortcuts" => {
            if json {
                "[]".into()
            } else {
                "".into()
            }
        }
        "plugin" => plugin_cmd(rest, json, state),
        "session" => session_cmd(rest, json, state),
        "debug" => debug_cmd(rest, json, state),
        "cheatsheet" => cheatsheet_cmd(json, state),
        "exit-safe-mode" => {
            if state.safe_mode {
                state.safe_mode = false;
                tracing::info!("Safe mode disabled via IPC exit-safe-mode");
                "ok".into()
            } else {
                "ok (not in safe mode)".into()
            }
        }
        _ => format!("error: unknown command '{verb}'"),
    }
}

// ─── Batch ───────────────────────────────────────────────────────────────────

fn handle_batch(cmds: &str, state: &mut GlobalState) -> String {
    cmds.split(';')
        .map(|c| handle(c.trim(), state))
        .collect::<Vec<_>>()
        .join("\n")
}

// ─── Version ─────────────────────────────────────────────────────────────────

fn version(json: bool) -> String {
    let v = env!("CARGO_PKG_VERSION");
    if json {
        format!(r#"{{"version":"{v}","commit":"","branch":"main"}}"#)
    } else {
        format!("GriddyWM v{v}")
    }
}

// ─── Reload ──────────────────────────────────────────────────────────────────

fn reload_cmd(args: &str, state: &mut GlobalState) -> String {
    match args.trim() {
        "theme" => {
            state.reload_theme();
            "ok".into()
        }
        _ => {
            state.reload_config_if_changed(true);
            "ok".into()
        }
    }
}

// ─── Dispatch ────────────────────────────────────────────────────────────────

fn dispatch_cmd(args_str: &str, json: bool, state: &mut GlobalState) -> String {
    let parts: Vec<String> = args_str.split_whitespace().map(|s| s.to_owned()).collect();
    let action_name = parts.first().map(|s| s.as_str()).unwrap_or("").to_owned();
    let action_args: Vec<String> = parts.into_iter().skip(1).collect();

    match dispatcher::parse_action(&action_name, &action_args) {
        Some(action) => {
            let quit = dispatcher::dispatch(action, state);
            if quit {
                state.should_exit = true;
            }
            if json { r#"{"ok":true}"# } else { "ok" }.into()
        }
        None => {
            if json {
                format!(r#"{{"ok":false,"error":"unknown action '{action_name}'"}}"#)
            } else {
                format!("error: unknown action '{action_name}'")
            }
        }
    }
}

// ─── Grid info / resize ───────────────────────────────────────────────────────

fn grid_cmd(args: &str, json: bool, state: &mut GlobalState) -> String {
    let (sub, rest) = split_verb(args.trim());
    match sub {
        "resize" => grid_resize(rest, json, state),
        _ => grid_info(json, state),
    }
}

fn grid_resize(args: &str, json: bool, state: &mut GlobalState) -> String {
    // Accept "4 4", "4x4", or "4,4".
    let s = args.trim().replace(['x', ','], " ");
    let parts: Vec<&str> = s.split_whitespace().collect();
    let (Some(cs), Some(rs)) = (parts.first(), parts.get(1)) else {
        return if json {
            r#"{"ok":false,"error":"usage: grid resize <cols> <rows>"}"#.into()
        } else {
            "error: usage: grid resize <cols> <rows>".into()
        };
    };
    let (Ok(new_cols), Ok(new_rows)) = (cs.parse::<u8>(), rs.parse::<u8>()) else {
        return if json {
            r#"{"ok":false,"error":"cols and rows must be 1–16"}"#.into()
        } else {
            "error: cols and rows must be 1–16".into()
        };
    };
    let (old_cols, old_rows) = state.grid.resize_grid(new_cols, new_rows);
    let new_cols = state.grid.cols;
    let new_rows = state.grid.rows;
    tracing::info!(
        "Grid resized {}x{} → {}x{}",
        old_cols,
        old_rows,
        new_cols,
        new_rows
    );
    if json {
        format!(r#"{{"ok":true,"cols":{new_cols},"rows":{new_rows}}}"#)
    } else {
        format!("grid resized to {new_cols}x{new_rows}")
    }
}

fn grid_info(json: bool, state: &GlobalState) -> String {
    let (col, row) = state.grid.focused;
    let cols = state.grid.cols;
    let rows = state.grid.rows;
    let wx = state.config.grid.wrap_x;
    let wy = state.config.grid.wrap_y;

    if json {
        format!(
            r#"{{"cols":{cols},"rows":{rows},"scope":"per-output","wrap_x":{wx},"wrap_y":{wy},"active":{{"col":{col},"row":{row},"monitor":"winit"}}}}"#
        )
    } else {
        format!("grid {cols}x{rows}  focused {col},{row}")
    }
}

// ─── Workspaces ──────────────────────────────────────────────────────────────

fn workspaces(json: bool, state: &GlobalState) -> String {
    let focused = state.grid.focused;
    let cols = state.grid.cols;
    let rows = state.grid.rows;

    if json {
        let entries: Vec<String> = (0..rows)
            .flat_map(|r| (0..cols).map(move |c| (c, r)))
            .map(|(c, r)| {
                let n = state.grid.windows.values().filter(|w| w.workspace == (c, r)).count();
                let f = (c, r) == focused;
                let name = state.grid.workspace_name(c, r);
                let name_json = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into());
                format!(
                    r#"{{"id":"winit:{c},{r}","col":{c},"row":{r},"monitor":"winit","name":{name_json},"windows":{n},"focused":{f}}}"#
                )
            })
            .collect();
        format!("[{}]", entries.join(","))
    } else {
        let lines: Vec<String> = (0..rows)
            .flat_map(|r| (0..cols).map(move |c| (c, r)))
            .map(|(c, r)| {
                let n = state
                    .grid
                    .windows
                    .values()
                    .filter(|w| w.workspace == (c, r))
                    .count();
                let mark = if (c, r) == focused { " *" } else { "" };
                let name = state.grid.workspace_name(c, r);
                let name_part = if name.is_empty() {
                    String::new()
                } else {
                    format!(" \"{name}\"")
                };
                format!("{c},{r}{name_part} [{n} windows]{mark}")
            })
            .collect();
        lines.join("\n")
    }
}

// ─── Active workspace ────────────────────────────────────────────────────────

fn active_workspace(json: bool, state: &GlobalState) -> String {
    let (col, row) = state.grid.focused;
    let n = state
        .grid
        .windows
        .values()
        .filter(|w| w.workspace == (col, row))
        .count();
    let name = state.grid.workspace_name(col, row);

    if json {
        let name_json = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into());
        format!(
            r#"{{"id":"winit:{col},{row}","col":{col},"row":{row},"monitor":"winit","name":{name_json},"windows":{n},"focused":true}}"#
        )
    } else {
        let name_part = if name.is_empty() {
            String::new()
        } else {
            format!(" \"{name}\"")
        };
        format!("{col},{row}{name_part} [{n} windows]")
    }
}

// ─── Windows ─────────────────────────────────────────────────────────────────

fn windows(json: bool, state: &GlobalState) -> String {
    let entries: Vec<String> = state
        .grid
        .windows
        .values()
        .map(|w| window_to_str(w, state, json))
        .collect();

    if json {
        format!("[{}]", entries.join(","))
    } else {
        entries.join("\n")
    }
}

// ─── Active window ───────────────────────────────────────────────────────────

fn active_window(json: bool, state: &GlobalState) -> String {
    let Some(id) = state.grid.focused_window else {
        return if json {
            "null".into()
        } else {
            "no focused window".into()
        };
    };
    let Some(w) = state.grid.windows.get(&id) else {
        return if json {
            "null".into()
        } else {
            "no focused window".into()
        };
    };
    window_to_str(w, state, json)
}

// ─── Monitors ────────────────────────────────────────────────────────────────

fn monitors(json: bool, state: &GlobalState) -> String {
    let w = state.grid.output_w;
    let h = state.grid.output_h;

    if json {
        format!(r#"[{{"name":"winit","width":{w},"height":{h},"scale":1.0,"focused":true}}]"#)
    } else {
        format!("winit {w}x{h} scale=1.0 *")
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn window_to_str(w: &crate::grid::window::Window, state: &GlobalState, json: bool) -> String {
    let geom = state.grid.compute_rect(w.id).unwrap_or_default();
    let (col, row) = w.workspace;
    let cur_state = state_name(w.current_state);
    let req_state = state_name(w.requested_state);
    let cur_slot = w.current_slot.map(slot_name);
    let req_slot = w.requested_slot.map(slot_name);
    let id = w.id;

    if json {
        let cur_slot_json = match cur_slot {
            Some(s) => format!("\"{s}\""),
            None => "null".into(),
        };
        let req_slot_json = match req_slot {
            Some(s) => format!("\"{s}\""),
            None => "null".into(),
        };
        let pid_json = match w.pid {
            Some(p) => p.to_string(),
            None => "null".into(),
        };
        let shader_json = match &w.shader {
            Some(s) => format!("\"{}\"", s.replace('"', "\\\"")),
            None => "null".into(),
        };
        format!(
            r#"{{"id":{id},"app_id":"{app_id}","title":"{title}","workspace":{{"col":{col},"row":{row},"monitor":"winit"}},"current_state":"{cur_state}","requested_state":"{req_state}","current_slot":{cur_slot_json},"requested_slot":{req_slot_json},"stack_index":{si},"geometry":[{gx},{gy},{gw},{gh}],"is_urgent":{urgent},"pid":{pid_json},"opacity":{opacity},"shader":{shader_json},"xwayland":false}}"#,
            app_id = w.app_id,
            title = w.title.replace('"', "\\\""),
            si = w.stack_index,
            gx = geom.x,
            gy = geom.y,
            gw = geom.w,
            gh = geom.h,
            urgent = w.is_urgent,
            opacity = w.opacity,
        )
    } else {
        let slot_str = cur_slot.unwrap_or("-");
        let urgent_str = if w.is_urgent { " urgent" } else { "" };
        format!(
            "id={id} app_id={app} title={title} ws={col},{row} state={cur_state} slot={slot_str}{urgent_str}",
            app = w.app_id,
            title = w.title,
        )
    }
}

pub fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::HalfLeft => "HalfLeft",
        Slot::HalfRight => "HalfRight",
        Slot::QuarterTL => "QuarterTL",
        Slot::QuarterTR => "QuarterTR",
        Slot::QuarterBL => "QuarterBL",
        Slot::QuarterBR => "QuarterBR",
    }
}

pub fn state_name(state: WindowState) -> &'static str {
    match state {
        WindowState::Tiled => "Tiled",
        WindowState::Floating => "Floating",
        WindowState::Fullscreen => "Fullscreen",
        WindowState::TotalFullscreen => "TotalFullscreen",
    }
}

fn split_verb(s: &str) -> (&str, &str) {
    match s.find(' ') {
        Some(i) => (&s[..i], s[i + 1..].trim()),
        None => (s, ""),
    }
}

// ─── keyword ────────────────────────────────────────────────────────────────

fn keyword_cmd(rest: &str, json: bool, state: &mut GlobalState) -> String {
    let result = keyword_apply(rest, state);
    if json {
        if result == "ok" {
            r#"{"ok":true}"#.into()
        } else {
            let escaped = result.replace('"', "\\\"");
            format!(r#"{{"ok":false,"error":"{escaped}"}}"#)
        }
    } else {
        result
    }
}

fn keyword_apply(rest: &str, state: &mut GlobalState) -> String {
    let (key, value) = split_verb(rest);
    if key.is_empty() {
        return "error: keyword requires <key> <value>".into();
    }
    // Apply a subset of runtime-overridable keys.
    match key {
        "animations.open_duration_ms" => {
            if let Ok(v) = value.parse::<u32>() {
                state.config.animations.open_duration_ms = v;
            }
            "ok".into()
        }
        "input.follow_mouse" => {
            use crate::config::types::FollowMouse;
            state.config.input.follow_mouse = match value {
                "off" => FollowMouse::Off,
                "strict" => FollowMouse::Strict,
                _ => FollowMouse::Loose,
            };
            "ok".into()
        }
        "input.cross_workspace_focus" => {
            state.config.input.cross_workspace_focus = value.parse().unwrap_or(true);
            state.grid.cross_workspace_focus = state.config.input.cross_workspace_focus;
            "ok".into()
        }
        "general.gaps_in" | "gaps.windows.inner_px" => {
            if let Ok(v) = value.parse::<i32>() {
                state.config.theme.gaps.windows.inner_px = v;
                state.grid.inner_gap_px = v;
            }
            "ok".into()
        }
        "general.gaps_out" | "gaps.windows.outer_px" => {
            if let Ok(v) = value.parse::<i32>() {
                state.config.theme.gaps.windows.outer_px = v;
                state.grid.outer_gap_px = v;
            }
            "ok".into()
        }
        "theme.osd.enabled" => {
            state.config.theme.osd.enabled = value.parse().unwrap_or(true);
            "ok".into()
        }
        "theme.osd.duration_ms" => {
            if let Ok(v) = value.parse::<u64>() {
                state.config.theme.osd.duration_ms = v;
            }
            "ok".into()
        }
        "input.warp_cursor_on_focus_change" => {
            state.config.input.warp_cursor_on_focus_change = value.parse().unwrap_or(false);
            "ok".into()
        }
        "input.warp_cursor_on_workspace_change" => {
            state.config.input.warp_cursor_on_workspace_change = value.parse().unwrap_or(false);
            "ok".into()
        }
        "debug.overlay" => {
            state.debug_overlay = value.parse().unwrap_or(false);
            "ok".into()
        }
        "fullscreen.performance_on_total_fullscreen" => {
            state.config.fullscreen.performance_on_total_fullscreen =
                value.parse().unwrap_or(false);
            "ok".into()
        }
        "shaders.screen" => {
            state.screen_shader = if value.is_empty() || value == "clear" {
                None
            } else {
                Some(value.to_owned())
            };
            "ok".into()
        }
        "animations.enabled" => {
            let enabled: bool = value.parse().unwrap_or(true);
            if !enabled {
                state.config.animations.open_duration_ms = 0;
                state.config.animations.close_duration_ms = 0;
            }
            "ok".into()
        }
        "animations.close_duration_ms" => {
            if let Ok(v) = value.parse::<u32>() {
                state.config.animations.close_duration_ms = v;
            }
            "ok".into()
        }
        "input.natural_scroll" => {
            state.config.input.natural_scroll = value.parse().unwrap_or(true);
            "ok".into()
        }
        "input.tap_to_click" => {
            state.config.input.tap_to_click = value.parse().unwrap_or(true);
            "ok".into()
        }
        "input.disable_while_typing" | "touchpad.disable_while_typing" => {
            state.config.input.touchpad.disable_while_typing = value.parse().unwrap_or(true);
            "ok".into()
        }
        "windows.focus_on_close" => {
            use crate::config::types::FocusOnClosePolicy;
            state.config.windows.focus_on_close = match value {
                "stack-next" => FocusOnClosePolicy::StackNext,
                "slot-neighbor" => FocusOnClosePolicy::SlotNeighbor,
                "none" => FocusOnClosePolicy::None,
                _ => FocusOnClosePolicy::LastFocused,
            };
            "ok".into()
        }
        "grid.workspace_sync" => {
            use crate::config::types::WorkspaceSyncMode;
            let synced = matches!(value, "synced");
            state.config.grid.workspace_sync = if synced {
                WorkspaceSyncMode::Synced
            } else {
                WorkspaceSyncMode::Unsynced
            };
            state.workspace_synced = synced;
            state
                .pending_events
                .push(crate::ipc::events::Event::WorkspaceSyncChanged { synced });
            "ok".into()
        }
        "grid.wrap_x" => {
            state.config.grid.wrap_x = value.parse().unwrap_or(false);
            let cfg = state.config.clone();
            state.grid.update_from_config(&cfg);
            "ok".into()
        }
        "grid.wrap_y" => {
            state.config.grid.wrap_y = value.parse().unwrap_or(false);
            let cfg = state.config.clone();
            state.grid.update_from_config(&cfg);
            "ok".into()
        }
        _ => {
            tracing::debug!(key = %key, value = %value, "keyword: unsupported key (ignoring)");
            "ok".into()
        }
    }
}

// ─── layers ──────────────────────────────────────────────────────────────────

fn layers_cmd(json: bool, state: &GlobalState) -> String {
    let items: Vec<String> = state
        .layer_surfaces
        .iter()
        .map(|ls| {
            let ns = &ls.namespace;
            let layer_name = format!("{:?}", ls.layer).to_lowercase();
            if json {
                format!(
                    r#"{{"namespace":{},"layer":{}}}"#,
                    serde_json::to_string(ns).unwrap_or_default(),
                    serde_json::to_string(&layer_name).unwrap_or_default()
                )
            } else {
                format!("{ns} ({layer_name})")
            }
        })
        .collect();
    if json {
        format!("[{}]", items.join(","))
    } else {
        items.join("\n")
    }
}

// ─── animations ─────────────────────────────────────────────────────────────

fn animations_info(json: bool, state: &GlobalState) -> String {
    let open_ms = state.config.animations.open_duration_ms;
    if json {
        format!(r#"{{"open_duration_ms":{open_ms}}}"#)
    } else {
        format!("open_duration_ms={open_ms}")
    }
}

// ─── getoption ───────────────────────────────────────────────────────────────

fn getoption_cmd(rest: &str, json: bool, state: &GlobalState) -> String {
    let key = rest.trim();
    let value = match key {
        "animations.open_duration_ms" => state.config.animations.open_duration_ms.to_string(),
        "input.follow_mouse" => format!("{:?}", state.config.input.follow_mouse).to_lowercase(),
        "input.cross_workspace_focus" => state.config.input.cross_workspace_focus.to_string(),
        "gaps.windows.inner_px" => state.config.theme.gaps.windows.inner_px.to_string(),
        "gaps.windows.outer_px" => state.config.theme.gaps.windows.outer_px.to_string(),
        "grid.cols" => state.grid.cols.to_string(),
        "grid.rows" => state.grid.rows.to_string(),
        "theme.osd.enabled" => state.config.theme.osd.enabled.to_string(),
        "theme.osd.duration_ms" => state.config.theme.osd.duration_ms.to_string(),
        "input.warp_cursor_on_focus_change" => {
            state.config.input.warp_cursor_on_focus_change.to_string()
        }
        "input.warp_cursor_on_workspace_change" => state
            .config
            .input
            .warp_cursor_on_workspace_change
            .to_string(),
        "input.pointer_constraint_break_key" => {
            state.config.input.pointer_constraint_break_key.clone()
        }
        "debug.overlay" => state.debug_overlay.to_string(),
        "fullscreen.performance_on_total_fullscreen" => state
            .config
            .fullscreen
            .performance_on_total_fullscreen
            .to_string(),
        "shaders.screen" => state.screen_shader.clone().unwrap_or_default(),
        "animations.close_duration_ms" => state.config.animations.close_duration_ms.to_string(),
        "input.natural_scroll" => state.config.input.natural_scroll.to_string(),
        "input.tap_to_click" => state.config.input.tap_to_click.to_string(),
        "input.disable_while_typing" => {
            state.config.input.touchpad.disable_while_typing.to_string()
        }
        "windows.focus_on_close" => {
            format!("{:?}", state.config.windows.focus_on_close).to_lowercase()
        }
        "windows.new_window.same_app" => state.config.windows.new_window.same_app.clone(),
        "windows.new_window.focus_steal_prevention" => state
            .config
            .windows
            .new_window
            .focus_steal_prevention
            .to_string(),
        "safe_mode" => state.safe_mode.to_string(),
        "version" => env!("CARGO_PKG_VERSION").into(),
        "grid.workspace_sync" => if state.workspace_synced {
            "synced"
        } else {
            "unsynced"
        }
        .into(),
        "grid.wrap_x" => state.grid.wrap_x().to_string(),
        "grid.wrap_y" => state.grid.wrap_y().to_string(),
        "windows.on_slot_conflict" => {
            format!("{:?}", state.config.windows.on_slot_conflict).to_lowercase()
        }
        "windows.slot_adaptation" => state.config.windows.slot_adaptation.to_string(),
        _ => return format!("error: unknown option '{key}'"),
    };
    if json {
        let esc = value.replace('"', "\\\"");
        format!(r#"{{"option":"{key}","value":"{esc}"}}"#)
    } else {
        format!("{key}={value}")
    }
}

// ─── setprop ─────────────────────────────────────────────────────────────────

fn setprop_cmd(rest: &str, state: &mut GlobalState) -> String {
    // setprop <window_id> <prop> <value>
    let mut parts = rest.splitn(3, ' ');
    let id_str = parts.next().unwrap_or("");
    let prop = parts.next().unwrap_or("");
    let value = parts.next().unwrap_or("");

    let Ok(id) = id_str.parse::<u64>() else {
        return "error: invalid window id".into();
    };
    let Some(w) = state.grid.windows.get_mut(&id) else {
        return format!("error: window {id} not found");
    };
    match prop {
        "is_urgent" => {
            w.is_urgent = value.parse().unwrap_or(false);
            "ok".into()
        }
        "sticky" | "pin" => {
            w.sticky = value.parse().unwrap_or(false);
            "ok".into()
        }
        "no_animations" => {
            w.no_animations = value.parse().unwrap_or(false);
            "ok".into()
        }
        "steal_focus" => {
            w.steal_focus = value.parse().unwrap_or(false);
            "ok".into()
        }
        "above_total_fullscreen" => {
            w.above_total_fullscreen = value.parse().unwrap_or(true);
            "ok".into()
        }
        "opacity" => match value.parse::<f32>() {
            Ok(v) if (0.0..=1.0).contains(&v) => {
                w.opacity = v;
                "ok".into()
            }
            _ => "error: opacity must be 0.0–1.0".into(),
        },
        "border_color" => {
            w.border_color = if value.is_empty() {
                None
            } else {
                Some(value.to_owned())
            };
            "ok".into()
        }
        "shader" => {
            w.shader = if value.is_empty() {
                None
            } else {
                Some(value.to_owned())
            };
            "ok".into()
        }
        _ => format!("error: unknown property '{prop}'"),
    }
}

// ─── session ─────────────────────────────────────────────────────────────────

fn session_cmd(rest: &str, json: bool, state: &mut GlobalState) -> String {
    let sub = rest.trim();
    match sub {
        "save" => {
            crate::session::save(state);
            if json {
                r#"{"ok":true}"#.into()
            } else {
                "ok".into()
            }
        }
        "clear" => {
            crate::session::clear();
            state.session = None;
            if json {
                r#"{"ok":true}"#.into()
            } else {
                "ok".into()
            }
        }
        "restore" => {
            // Reload session from disk; new windows will pick up the restored hints.
            state.session = crate::session::load();
            let count = state.session.as_ref().map(|s| s.windows.len()).unwrap_or(0);
            if json {
                format!(r#"{{"windows_loaded":{count}}}"#)
            } else {
                format!("loaded {count} window entries")
            }
        }
        "status" => {
            let count = state.session.as_ref().map(|s| s.windows.len()).unwrap_or(0);
            if json {
                format!(
                    r#"{{"pending_restores":{count},"safe_mode":{}}}"#,
                    state.safe_mode
                )
            } else {
                format!("pending_restores={count} safe_mode={}", state.safe_mode)
            }
        }
        _ => format!("error: unknown session subcommand '{sub}'. Use save|clear|restore|status"),
    }
}

// ─── debug ───────────────────────────────────────────────────────────────────

fn debug_cmd(rest: &str, json: bool, state: &mut GlobalState) -> String {
    let sub = rest.trim();
    match sub {
        "surfaces" | "tree" => {
            // Surface tree dump: list all windows with their state/slot info.
            let entries: Vec<String> = state
                .grid
                .windows
                .values()
                .map(|w| {
                    if json {
                        format!(
                            r#"{{"id":{},"app_id":{},"state":"{:?}","slot":{:?},"ws":[{},{}]}}"#,
                            w.id,
                            serde_json::to_string(&w.app_id).unwrap_or_default(),
                            w.current_state,
                            w.current_slot.map(|s| format!("{s:?}")),
                            w.workspace.0,
                            w.workspace.1
                        )
                    } else {
                        format!(
                            "id={} app={:?} state={:?} slot={:?} ws={},{}",
                            w.id,
                            w.app_id,
                            w.current_state,
                            w.current_slot,
                            w.workspace.0,
                            w.workspace.1
                        )
                    }
                })
                .collect();
            if json {
                format!("[{}]", entries.join(","))
            } else {
                entries.join("\n")
            }
        }
        "fps" => {
            let uptime = state.clock.now().as_millis();
            if json {
                format!(
                    r#"{{"uptime_ms":{uptime},"output_size":[{},{}]}}"#,
                    state.grid.output_w, state.grid.output_h
                )
            } else {
                format!(
                    "uptime={}ms output={}x{}",
                    uptime, state.grid.output_w, state.grid.output_h
                )
            }
        }
        "safe_mode" => {
            if json {
                format!(r#"{{"safe_mode":{}}}"#, state.safe_mode)
            } else {
                format!("safe_mode={}", state.safe_mode)
            }
        }
        "" | "help" => "debug subcommands: surfaces|tree, fps, safe_mode".into(),
        _ => format!("error: unknown debug subcommand '{sub}'. Use: surfaces, fps, safe_mode"),
    }
}

// ─── Notify ──────────────────────────────────────────────────────────────────

fn notify_cmd(rest: &str, state: &mut GlobalState) -> String {
    // On first notification request, check whether org.freedesktop.Notifications
    // is reachable on the session D-Bus (§14.2).  Uses busctl if available;
    // silently skips the check if busctl is not installed.
    if !state.notification_daemon_checked {
        state.notification_daemon_checked = true;
        let reachable = std::process::Command::new("busctl")
            .args(["--user", "ping", "org.freedesktop.Notifications"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(true); // if busctl absent, assume daemon present
        if !reachable {
            tracing::warn!(
                "org.freedesktop.Notifications not reachable — emitting notification_daemon_missing"
            );
            state
                .pending_events
                .push(crate::ipc::events::Event::NotificationDaemonMissing);
            state.show_osd("notification-daemon-missing");
        }
    }
    tracing::info!("IPC notify: {rest}");
    "ok".into()
}

// ─── Cheatsheet ───────────────────────────────────────────────────────────────

fn cheatsheet_cmd(json: bool, state: &GlobalState) -> String {
    let entries = state.keybind_table.for_cheatsheet();
    if json {
        let items: Vec<String> = entries.iter().map(|(mods, key, action, locked, global)| {
            let combo = if mods.is_empty() {
                key.clone()
            } else {
                format!("{mods}+{key}")
            };
            format!(
                r#"{{"combo":{},"mods":{},"key":{},"action":{},"locked":{locked},"global":{global}}}"#,
                serde_json::to_string(&combo).unwrap_or_default(),
                serde_json::to_string(mods).unwrap_or_default(),
                serde_json::to_string(key).unwrap_or_default(),
                serde_json::to_string(action).unwrap_or_default(),
            )
        }).collect();
        format!("[{}]", items.join(","))
    } else {
        // Human-readable table: combo → action
        let lines: Vec<String> = entries
            .iter()
            .map(|(mods, key, action, locked, global)| {
                let combo = if mods.is_empty() {
                    key.clone()
                } else {
                    format!("{mods}+{key}")
                };
                let flags = match (locked, global) {
                    (true, true) => " [locked,global]",
                    (true, false) => " [locked]",
                    (false, true) => " [global]",
                    (false, false) => "",
                };
                format!("{combo:<30} {action}{flags}")
            })
            .collect();
        lines.join("\n")
    }
}

// ─── shaders ─────────────────────────────────────────────────────────────────

fn shaders_cmd(json: bool, state: &GlobalState) -> String {
    let screen = state.screen_shader.as_deref().unwrap_or("");
    if json {
        format!(
            r#"[{{"category":"screen","path":{}}}]"#,
            serde_json::to_string(screen).unwrap_or_default()
        )
    } else if screen.is_empty() {
        "screen: (none)".into()
    } else {
        format!("screen: {screen}")
    }
}

// ─── plugin ──────────────────────────────────────────────────────────────────

fn plugin_cmd(rest: &str, json: bool, state: &mut GlobalState) -> String {
    let mut parts = rest.splitn(2, ' ');
    let sub = parts.next().unwrap_or("").trim();
    let arg = parts.next().unwrap_or("").trim();

    match sub {
        "list" => {
            #[cfg(feature = "plugin-abi")]
            {
                let items: Vec<String> = state
                    .plugins
                    .iter()
                    .map(|p| {
                        let name = p.name();
                        if json {
                            format!(
                                r#"{{"name":{}}}"#,
                                serde_json::to_string(name).unwrap_or_default()
                            )
                        } else {
                            name.to_owned()
                        }
                    })
                    .collect();
                if json {
                    format!("[{}]", items.join(","))
                } else {
                    items.join("\n")
                }
            }
            #[cfg(not(feature = "plugin-abi"))]
            {
                if json {
                    "[]".into()
                } else {
                    "(plugins feature not enabled)".into()
                }
            }
        }
        "load" => {
            if arg.is_empty() {
                return "error: plugin load <path>".into();
            }
            #[cfg(feature = "plugin-abi")]
            {
                let cfg = crate::plugins::PluginConfig {
                    path: arg.to_owned(),
                    enabled: true,
                    config: String::new(),
                };
                match crate::plugins::load_plugin(&cfg) {
                    Ok(p) => {
                        let name = p.name().to_owned();
                        state.plugins.push(p);
                        state
                            .pending_events
                            .push(crate::ipc::events::Event::PluginLoaded { name: name.clone() });
                        if json {
                            format!(
                                r#"{{"ok":true,"name":{}}}"#,
                                serde_json::to_string(&name).unwrap_or_default()
                            )
                        } else {
                            format!("ok: loaded {name}")
                        }
                    }
                    Err(e) => {
                        state
                            .pending_events
                            .push(crate::ipc::events::Event::PluginError {
                                name: arg.to_owned(),
                                message: e.clone(),
                            });
                        if json {
                            format!(
                                r#"{{"ok":false,"error":{}}}"#,
                                serde_json::to_string(&e).unwrap_or_default()
                            )
                        } else {
                            format!("error: {e}")
                        }
                    }
                }
            }
            #[cfg(not(feature = "plugin-abi"))]
            {
                "error: plugin-abi feature not compiled in".into()
            }
        }
        "unload" => {
            if arg.is_empty() {
                return "error: plugin unload <name>".into();
            }
            #[cfg(feature = "plugin-abi")]
            {
                let before = state.plugins.len();
                state.plugins.retain(|p| p.name() != arg);
                if state.plugins.len() < before {
                    "ok".into()
                } else {
                    format!("error: plugin '{arg}' not found")
                }
            }
            #[cfg(not(feature = "plugin-abi"))]
            {
                "error: plugin-abi feature not compiled in".into()
            }
        }
        _ => "error: plugin subcommands: list, load <path>, unload <name>".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_verb_with_space() {
        let (verb, rest) = split_verb("dispatch workspace-left");
        assert_eq!(verb, "dispatch");
        assert_eq!(rest, "workspace-left");
    }

    #[test]
    fn split_verb_no_space() {
        let (verb, rest) = split_verb("version");
        assert_eq!(verb, "version");
        assert_eq!(rest, "");
    }

    #[test]
    fn split_verb_multiple_words() {
        let (verb, rest) = split_verb("grid resize 4 4");
        assert_eq!(verb, "grid");
        assert_eq!(rest, "resize 4 4");
    }

    #[test]
    fn split_verb_empty() {
        let (verb, rest) = split_verb("");
        assert_eq!(verb, "");
        assert_eq!(rest, "");
    }

    #[test]
    fn slot_name_roundtrip() {
        use crate::grid::window::Slot;
        assert_eq!(slot_name(Slot::HalfLeft), "HalfLeft");
        assert_eq!(slot_name(Slot::HalfRight), "HalfRight");
        assert_eq!(slot_name(Slot::QuarterTL), "QuarterTL");
        assert_eq!(slot_name(Slot::QuarterTR), "QuarterTR");
        assert_eq!(slot_name(Slot::QuarterBL), "QuarterBL");
        assert_eq!(slot_name(Slot::QuarterBR), "QuarterBR");
    }

    #[test]
    fn state_name_roundtrip() {
        use crate::grid::window::WindowState;
        assert_eq!(state_name(WindowState::Tiled), "Tiled");
        assert_eq!(state_name(WindowState::Floating), "Floating");
        assert_eq!(state_name(WindowState::Fullscreen), "Fullscreen");
        assert_eq!(state_name(WindowState::TotalFullscreen), "TotalFullscreen");
    }
}
