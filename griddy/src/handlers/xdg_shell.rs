use smithay::{
    delegate_foreign_toplevel_list, delegate_xdg_decoration, delegate_xdg_shell,
    reexports::wayland_server::protocol::wl_seat::WlSeat,
    utils::{Serial, SERIAL_COUNTER},
    wayland::{
        foreign_toplevel_list::{ForeignToplevelListHandler, ForeignToplevelListState},
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
            decoration::XdgDecorationHandler,
        },
    },
};
use wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecoMode;
use wayland_protocols::xdg::shell::server::xdg_toplevel;
use wayland_server::Resource;

use crate::animate::WindowAnim;
use crate::grid::{
    AddWindowResult, PlacementHints,
    window::{Rect, Slot, WindowState},
};
use crate::ipc::{commands as ipc_cmds, events::Event};
use crate::state::GlobalState;

impl XdgShellHandler for GlobalState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // Read the app_id/title and parent surface from the pending xdg state.
        let (app_id, title, parent_surface) = {
            use smithay::wayland::compositor::with_states;
            use smithay::wayland::shell::xdg::XdgToplevelSurfaceRoleAttributes;
            with_states(surface.wl_surface(), |states| {
                let attrs = states
                    .data_map
                    .get::<std::sync::Mutex<XdgToplevelSurfaceRoleAttributes>>()
                    .map(|m| m.lock().unwrap());
                let app_id = attrs
                    .as_ref()
                    .and_then(|a| a.app_id.clone())
                    .unwrap_or_default();
                let title = attrs
                    .as_ref()
                    .and_then(|a| a.title.clone())
                    .unwrap_or_default();
                let parent = attrs.as_ref().and_then(|a| a.parent.clone());
                (app_id, title, parent)
            })
        };

        // Evaluate window rules (§10). Last-wins cascade.
        let mut hints = evaluate_rules(
            &self.config.rules,
            &app_id,
            &title,
            false,
            Some(self.grid.focused),
            false,
        );

        // Apply any pending spawn-hint (from spawn-floating / spawn-in-slot /
        // spawn-on-workspace / spawn-stacked dispatchers). Spawn hint wins.
        if let Some(sh) = self.spawn_hint.take() {
            if sh.state.is_some() {
                hints.state = sh.state;
            }
            if sh.slot.is_some() {
                hints.slot = sh.slot;
            }
            if sh.workspace.is_some() {
                hints.workspace = sh.workspace;
            }
            if sh.floating_geom.is_some() {
                hints.floating_geom = sh.floating_geom;
            }
            if sh.no_focus {
                hints.no_focus = true;
            }
        }

        // §6.9 Transient dialogs: if xdg_toplevel.set_parent is set, float centered on parent.
        if hints.state.is_none() && let Some(ref par_surface) = parent_surface {
            let par_id = par_surface.id();
            if let Some(par_wid) = self.grid.window_id_for_surface(&par_id)
                && let Some(par_rect) = self.grid.compute_rect(par_wid)
            {
                let par_ws = self.grid.windows.get(&par_wid).map(|w| w.workspace);
                // Center the dialog on the parent window (assume 600×400 default).
                let dw = 600.min(par_rect.w);
                let dh = 400.min(par_rect.h);
                let dx = par_rect.x + (par_rect.w - dw) / 2;
                let dy = par_rect.y + (par_rect.h - dh) / 2;
                hints.state = Some(WindowState::Floating);
                hints.floating_geom = Some(Rect::new(dx, dy, dw, dh));
                if let Some(ws) = par_ws {
                    hints.workspace = Some(ws);
                }
            }
        }

        // §22.1 Session restore: if no rule set workspace/slot yet, check saved session.
        if hints.workspace.is_none() && hints.slot.is_none()
            && let Some(ref mut sess) = self.session
            && let Some(entry) = sess.take_hint(&app_id, &title)
        {
            hints.workspace = Some((entry.workspace_col, entry.workspace_row));
            if let Some(ref slot_name) = entry.slot {
                hints.slot = parse_slot(slot_name);
            }
            tracing::debug!(
                app_id,
                workspace_col = entry.workspace_col,
                workspace_row = entry.workspace_row,
                "Session restore applied"
            );
        }

        // §6.9 same_app placement policy: if a window with the same app_id is already
        // on the focused workspace and no other placement hint is set, stack with it.
        let policy_matched: &str;
        if hints.state.is_none() && hints.slot.is_none() && hints.workspace.is_none() {
            let same_app_policy = &self.config.windows.new_window.same_app;
            let focused_ws = self.grid.focused;
            let existing_slot = self
                .grid
                .windows
                .values()
                .filter(|w| w.workspace == focused_ws && w.app_id == app_id)
                .filter_map(|w| w.current_slot)
                .next();
            if same_app_policy == "stack-with-focused" {
                if let Some(slot) = existing_slot {
                    hints.slot = Some(slot);
                    policy_matched = "same_app";
                } else {
                    policy_matched = "default";
                }
            } else {
                policy_matched = "default";
            }
        } else {
            policy_matched = "rule";
        }

        // §22.4 Window swallowing: get client PID, store it, then check swallow rules.
        let client_pid: Option<u32> = surface
            .wl_surface()
            .client()
            .and_then(|c| c.get_credentials(&self.display_handle).ok())
            .map(|cred| cred.pid as u32);
        hints.pid = client_pid;

        // Check if any rule for this window has a `swallow` pattern.
        let swallow_pat = self
            .config
            .rules
            .iter()
            .filter(|r| r.matches(&app_id, &title, false, None, false))
            .find_map(|r| r.action.swallow.clone());

        if let (Some(pat), Some(new_pid)) = (swallow_pat, client_pid) {
            // Find the parent PID of the new window.
            if let Some(ppid) = read_ppid(new_pid) {
                // Find an existing window owned by the ppid whose app_id matches pat.
                let swallow_candidate = self
                    .grid
                    .windows
                    .values()
                    .find(|w| {
                        w.pid.map(|p| p == ppid).unwrap_or(false)
                            && crate::config::rules::glob_match_pub(&pat, &w.app_id)
                    })
                    .map(|w| w.id);
                if let Some(victim_id) = swallow_candidate {
                    // Take the victim's slot for the new window.
                    let victim_slot = self
                        .grid
                        .windows
                        .get(&victim_id)
                        .and_then(|w| w.current_slot);
                    let victim_ws = self.grid.windows.get(&victim_id).map(|w| w.workspace);
                    if let (Some(slot), Some(ws)) = (victim_slot, victim_ws) {
                        hints.slot = Some(slot);
                        hints.workspace = Some(ws);
                        hints.swallows = Some(victim_id);
                        tracing::debug!(new_pid, ppid, victim_id, "Swallowing window (§22.4)");
                    }
                }
            }
        }

        let no_focus_hint = hints.no_focus;
        let prev_focused = self.grid.focused_window;

        // Phase 1: placement policy + conflict resolution.
        // Plugin: window_pre_open (§13) — window not yet assigned an ID.
        {
            let app_id_cstr = std::ffi::CString::new(app_id.as_str()).unwrap_or_default();
            let title_cstr = std::ffi::CString::new(title.as_str()).unwrap_or_default();
            let info = crate::plugins::GriddyWindowInfo {
                id: 0,
                app_id: app_id_cstr.as_ptr(),
                title: title_cstr.as_ptr(),
                x: self.grid.usable_x,
                y: self.grid.usable_y,
                w: self.grid.usable_w,
                h: self.grid.usable_h,
            };
            for plugin in &mut self.plugins {
                plugin.call_window_pre_open(&info);
            }
        }

        let AddWindowResult {
            id,
            slot_adapted,
            state_adapted,
        } = self.grid.add_window(surface.clone(), hints);

        // Store the app_id/title from the rule evaluation.
        self.grid
            .update_window_metadata(id, app_id.to_string(), title.to_string());

        tracing::debug!(window_id = id, "New toplevel mapped");

        // Start open fade-in animation (skipped by rule no_animations, §22.13).
        let skip_anim = self
            .grid
            .windows
            .get(&id)
            .map(|w| w.no_animations)
            .unwrap_or(false);
        let open_ms = if skip_anim {
            0
        } else {
            self.anim_duration(self.config.animations.open_duration_ms as u64)
        };
        if open_ms > 0 {
            self.open_anims.insert(id, WindowAnim::open(open_ms));
        }

        // Register with ext-foreign-toplevel-list-v1.
        let handle = self
            .foreign_toplevel_state
            .new_toplevel::<GlobalState>(title.clone(), app_id.clone());
        self.toplevel_handles.insert(id, handle);

        // Register with wlr-foreign-toplevel-management-unstable-v1.
        self.wlr_new_toplevel(id, title.clone(), app_id.clone());

        // Emit window_opened and window_placed events.
        if let Some(w) = self.grid.windows.get(&id) {
            let (wc, wr) = w.workspace;
            let slot_str = w
                .current_slot
                .map(ipc_cmds::slot_name)
                .unwrap_or("")
                .to_owned();
            self.pending_events.push(Event::WindowOpened {
                id,
                app_id: app_id.to_string(),
                workspace_col: wc,
                workspace_row: wr,
                state: ipc_cmds::state_name(w.current_state).to_owned(),
                slot: slot_str.clone(),
            });
            self.pending_events.push(Event::WindowPlaced {
                id,
                policy_matched: policy_matched.to_owned(),
                slot_assigned: slot_str,
            });
            // Emit slot/state adaptation events if conflict resolution adapted the placement.
            if let Some((requested, actual)) = slot_adapted {
                self.pending_events.push(Event::WindowSlotAdapted {
                    id,
                    requested: ipc_cmds::slot_name(requested).to_owned(),
                    actual: ipc_cmds::slot_name(actual).to_owned(),
                });
            }
            if let Some((req_state, actual_state, actual_slot)) = state_adapted {
                self.pending_events.push(Event::WindowStateAdapted {
                    id,
                    requested_state: ipc_cmds::state_name(req_state).to_owned(),
                    actual_state: ipc_cmds::state_name(actual_state).to_owned(),
                    actual_slot: actual_slot
                        .map(ipc_cmds::slot_name)
                        .unwrap_or("")
                        .to_owned(),
                });
            }
        }

        // Focus stealing prevention (§22.5): if there was already a focused window
        // and the new window stole focus, revert and mark the new window urgent.
        // Windows with steal_focus = true in their rule bypass this check.
        let steal_focus_allowed = self
            .grid
            .windows
            .get(&id)
            .map(|w| w.steal_focus)
            .unwrap_or(false);
        if self.config.windows.new_window.focus_steal_prevention
            && prev_focused.is_some()
            && !no_focus_hint
            && !steal_focus_allowed
            && self.grid.focused_window == Some(id)
        {
            self.grid.focused_window = prev_focused;
            if let Some(w) = self.grid.windows.get_mut(&id) {
                w.is_urgent = true;
            }
            self.pending_events.push(Event::Urgent { id });
            tracing::debug!(window_id = id, "Focus steal blocked — window marked urgent");
        }

        // Update keyboard focus to the new window (unless the rule suppressed it or blocked).
        if self.grid.focused_window == Some(id) && let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, Some(surface.wl_surface().clone()), SERIAL_COUNTER.next_serial());
        }
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
    }

    fn move_request(&mut self, _surface: ToplevelSurface, _seat: WlSeat, _serial: Serial) {}

    fn resize_request(
        &mut self,
        _surface: ToplevelSurface,
        _seat: WlSeat,
        _serial: Serial,
        _edges: xdg_toplevel::ResizeEdge,
    ) {
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        surface.send_configure();
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        surface.send_configure();
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
    ) {
        surface.send_configure();
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        surface.send_configure();
    }

    fn minimize_request(&mut self, _surface: ToplevelSurface) {}

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let surface_id = surface.wl_surface().id();
        if let Some(id) = self.grid.remove_window(&surface_id) {
            tracing::debug!(window_id = id, "Toplevel destroyed");
            self.open_anims.remove(&id);

            // Plugin: window_post_close (§13)
            {
                let empty = std::ffi::CString::default();
                let info = crate::plugins::GriddyWindowInfo {
                    id: id as u32,
                    app_id: empty.as_ptr(),
                    title: empty.as_ptr(),
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                };
                for plugin in &mut self.plugins {
                    plugin.call_window_post_close(&info);
                }
            }

            // Remove foreign toplevel handle (ext-foreign-toplevel-list-v1).
            if let Some(handle) = self.toplevel_handles.remove(&id) {
                self.foreign_toplevel_state.remove_toplevel(&handle);
            }
            // Notify wlr-foreign-toplevel-management clients.
            self.wlr_remove_toplevel(id);
            self.pending_events.push(Event::WindowClosed { id });
            if let Some(keyboard) = self.seat.get_keyboard() {
                let new_surface = self.grid.focused_surface().cloned();
                keyboard.set_focus(self, new_surface, SERIAL_COUNTER.next_serial());
            }
        }
    }

    fn popup_destroyed(&mut self, _surface: PopupSurface) {}
}

delegate_xdg_shell!(GlobalState);

// ─── Xdg decoration ──────────────────────────────────────────────────────────

impl XdgDecorationHandler for GlobalState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecoMode::ServerSide);
        });
        toplevel.send_configure();
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecoMode) {
        // Always enforce server-side decorations (our border rendering handles this).
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecoMode::ServerSide);
        });
        toplevel.send_configure();
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecoMode::ServerSide);
        });
        toplevel.send_configure();
    }
}

delegate_xdg_decoration!(GlobalState);

// ─── Foreign toplevel list ────────────────────────────────────────────────────

impl ForeignToplevelListHandler for GlobalState {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.foreign_toplevel_state
    }
}

delegate_foreign_toplevel_list!(GlobalState);

// ─── Rule evaluation ─────────────────────────────────────────────────────────

fn evaluate_rules(
    rules: &[crate::config::rules::Rule],
    app_id: &str,
    title: &str,
    is_xwayland: bool,
    workspace: Option<(u8, u8)>,
    fullscreen_request: bool,
) -> PlacementHints {
    let mut hints = PlacementHints::default();
    // above_total_fullscreen starts unset (None → defaults to true in add_window).

    for rule in rules {
        if !rule.matches(app_id, title, is_xwayland, workspace, fullscreen_request) {
            continue;
        }
        let a = &rule.action;
        if let Some(slot_str) = &a.slot {
            hints.slot = parse_slot(slot_str);
        }
        if let Some(state_str) = &a.state {
            hints.state = parse_state(state_str);
        }
        if let Some([col, row]) = a.workspace {
            hints.workspace = Some((col, row));
        }
        if let Some([x, y, w, h]) = a.floating_geom {
            hints.floating_geom = Some(Rect::new(x, y, w, h));
        }
        if a.no_focus {
            hints.no_focus = true;
        }
        if a.pin || a.sticky {
            hints.pin = true;
        }
        if let Some(op) = a.opacity {
            hints.opacity = Some(op);
        }
        if a.no_animations {
            hints.no_animations = true;
        }
        // above_total_fullscreen: only override when explicitly set to false in any rule.
        if !a.above_total_fullscreen {
            hints.above_total_fullscreen = Some(false);
        }
        if let Some(ref s) = a.border_color {
            hints.border_color = Some(s.clone());
        }
        if let Some(ref s) = a.shader {
            hints.shader = Some(s.clone());
        }
        if let Some(ref s) = a.min_size_overflow {
            hints.min_size_overflow = Some(s.clone());
        }
        if a.steal_focus {
            hints.steal_focus = true;
        }
    }

    hints
}

fn parse_slot(s: &str) -> Option<Slot> {
    match s {
        "half-left" | "HalfLeft" => Some(Slot::HalfLeft),
        "half-right" | "HalfRight" => Some(Slot::HalfRight),
        "quarter-tl" | "QuarterTL" => Some(Slot::QuarterTL),
        "quarter-tr" | "QuarterTR" => Some(Slot::QuarterTR),
        "quarter-bl" | "QuarterBL" => Some(Slot::QuarterBL),
        "quarter-br" | "QuarterBR" => Some(Slot::QuarterBR),
        _ => {
            tracing::warn!(slot = %s, "Unknown slot in rule — ignoring");
            None
        }
    }
}

fn parse_state(s: &str) -> Option<WindowState> {
    match s {
        "tiled" => Some(WindowState::Tiled),
        "floating" => Some(WindowState::Floating),
        "fullscreen" => Some(WindowState::Fullscreen),
        "total-fullscreen" => Some(WindowState::TotalFullscreen),
        _ => {
            tracing::warn!(state = %s, "Unknown state in rule — ignoring");
            None
        }
    }
}

// ─── §22.4 Window swallowing helpers ─────────────────────────────────────────

/// Read the parent PID of `pid` from `/proc/<pid>/status` on Linux.
/// Returns None if the file is unreadable or PPid line is missing.
fn read_ppid(pid: u32) -> Option<u32> {
    let content = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("PPid:\t") {
            return rest.trim().parse().ok();
        }
    }
    None
}
