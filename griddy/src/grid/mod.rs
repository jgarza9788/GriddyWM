//! Grid model: NxN workspace grid, window registry, slot assignment, navigation.

pub mod conflict;
pub mod focus;
pub mod layout;
pub mod placement;
pub mod window;
pub mod workspace;

use std::collections::{HashMap, VecDeque};

use smithay::{reexports::wayland_server::backend::ObjectId, wayland::shell::xdg::ToplevelSurface};
use wayland_protocols::xdg::shell::server::xdg_toplevel;
use wayland_server::Resource;

use crate::config::types::{Config, ConflictPolicy, FocusOnClosePolicy};
use window::{Rect, Slot, Window, WindowId, WindowState};
use workspace::Workspace;

pub use focus::FocusDirection;

// ─── Placement hints ──────────────────────────────────────────────────────────

/// Overrides from window rules (§10) that pre-empt the normal placement policy.
#[derive(Debug, Default)]
pub struct PlacementHints {
    /// Force placement on this workspace instead of the focused one.
    pub workspace: Option<(u8, u8)>,
    /// Override the initial slot selection.
    pub slot: Option<Slot>,
    /// Override the initial window state.
    pub state: Option<WindowState>,
    /// Initial floating geometry [x, y, w, h].
    pub floating_geom: Option<Rect>,
    /// Don't give focus to this window when it opens.
    pub no_focus: bool,
    /// Client PID (from Wayland socket credentials). Used for swallowing (§22.4).
    pub pid: Option<u32>,
    /// If this window should swallow an existing one, the swallowed WindowId.
    pub swallows: Option<WindowId>,
    /// Window opacity override (0.0–1.0; None = use theme default).
    pub opacity: Option<f32>,
    /// Pin to all workspaces as floating.
    pub pin: bool,
    /// Alias for pin (sticky = visible on every workspace).
    pub sticky: bool,
    /// Skip all animations for this window.
    pub no_animations: bool,
    /// Render above TotalFullscreen (None = true per default).
    pub above_total_fullscreen: Option<bool>,
    /// Custom border color override.
    pub border_color: Option<String>,
    /// Path to per-window GLSL shader.
    pub shader: Option<String>,
    /// Min-size overflow policy: "float" | "clip" | "ignore".
    pub min_size_overflow: Option<String>,
    /// Allow this window to steal focus even when focus_steal_prevention is on (§22.5).
    pub steal_focus: bool,
}

// ─── Grid ─────────────────────────────────────────────────────────────────────

pub struct Grid {
    pub cols: u8,
    pub rows: u8,
    /// Indexed [row][col].
    workspaces: Vec<Vec<Workspace>>,
    /// Focused workspace coordinates.
    pub focused: (u8, u8),
    /// Workspace navigation history (most-recent first).
    history: VecDeque<(u8, u8)>,
    /// Forward history for workspace-forward (cleared on manual switch).
    forward_history: VecDeque<(u8, u8)>,

    /// All windows keyed by stable WindowId.
    pub windows: HashMap<WindowId, Window>,
    /// Fast reverse lookup: wayland ObjectId → WindowId.
    surface_to_window: HashMap<ObjectId, WindowId>,
    /// Currently focused window (receives keyboard input).
    pub focused_window: Option<WindowId>,
    next_id: u64,

    /// Full output pixel dimensions (updated on resize).
    pub output_w: i32,
    pub output_h: i32,

    /// Usable area offset + size after subtracting exclusive-zone layer surfaces.
    /// Tiled windows are laid out within this area.
    pub usable_x: i32,
    pub usable_y: i32,
    pub usable_w: i32,
    pub usable_h: i32,

    // ── Gap values (from theme.toml §8.5) ───────────────────────────────────
    pub inner_gap_px: i32,
    pub outer_gap_px: i32,

    // ── Cached config values ─────────────────────────────────────────────────
    wrap_x: bool,
    wrap_y: bool,
    conflict_policy: ConflictPolicy,
    slot_adaptation: bool,
    fullscreen_adaptation: bool,
    fullscreen_auto_restore: bool,
    focus_steals_demotes: bool,
    focus_on_close: FocusOnClosePolicy,
    pub cross_workspace_focus: bool,
}

/// Return value of `Grid::move_window_direction_ex` — carries skip info for IPC events (§6.7.1).
pub enum MoveDirectionResult {
    /// Window moved; if `skipped` is true it bypassed one or more TF-protected workspaces.
    Moved {
        requested: (u8, u8),
        actual: (u8, u8),
        skipped: bool,
    },
    /// Every workspace in this direction is TF-protected — move refused.
    Refused,
    /// Mover's state is always allowed (Floating / TotalFullscreen); moved directly.
    MovedDirect { target: (u8, u8) },
}

/// Adaptation info returned by `Grid::add_window` alongside the WindowId.
pub struct AddWindowResult {
    pub id: WindowId,
    /// Slot adaptation: (requested_slot, actual_slot) when Half→Quarter adapted (§6.5.1).
    pub slot_adapted: Option<(Slot, Slot)>,
    /// State adaptation: (requested_state, actual_state, actual_slot) when Fullscreen→Tiled (§6.5.1).
    pub state_adapted: Option<(WindowState, WindowState, Option<Slot>)>,
}

impl Grid {
    pub fn new(config: &Config, output_w: i32, output_h: i32) -> Self {
        let cols = config.grid.cols.max(1);
        let rows = config.grid.rows.max(1);
        let workspaces = (0..rows)
            .map(|r| (0..cols).map(|c| Workspace::new(c, r)).collect())
            .collect();
        let focused = (
            config.grid.default[0].min(cols - 1),
            config.grid.default[1].min(rows - 1),
        );

        Self {
            cols,
            rows,
            workspaces,
            focused,
            history: VecDeque::new(),
            forward_history: VecDeque::new(),
            windows: HashMap::new(),
            surface_to_window: HashMap::new(),
            focused_window: None,
            next_id: 1,
            output_w,
            output_h,
            usable_x: 0,
            usable_y: 0,
            usable_w: output_w,
            usable_h: output_h,
            inner_gap_px: config.theme.gaps.windows.inner_px,
            outer_gap_px: config.theme.gaps.windows.outer_px,
            wrap_x: config.grid.wrap_x,
            wrap_y: config.grid.wrap_y,
            conflict_policy: config.windows.on_slot_conflict.clone(),
            slot_adaptation: config.windows.slot_adaptation,
            fullscreen_adaptation: config.windows.fullscreen_adaptation,
            fullscreen_auto_restore: config.windows.fullscreen_auto_restore,
            focus_steals_demotes: config.windows.focus_steals_demotes,
            focus_on_close: config.windows.focus_on_close.clone(),
            cross_workspace_focus: config.input.cross_workspace_focus,
        }
    }

    /// Apply updated config values without resetting the window/workspace state.
    pub fn update_from_config(&mut self, config: &Config) {
        self.inner_gap_px = config.theme.gaps.windows.inner_px;
        self.outer_gap_px = config.theme.gaps.windows.outer_px;
        self.wrap_x = config.grid.wrap_x;
        self.wrap_y = config.grid.wrap_y;
        self.conflict_policy = config.windows.on_slot_conflict.clone();
        self.slot_adaptation = config.windows.slot_adaptation;
        self.fullscreen_adaptation = config.windows.fullscreen_adaptation;
        self.fullscreen_auto_restore = config.windows.fullscreen_auto_restore;
        self.focus_steals_demotes = config.windows.focus_steals_demotes;
        self.focus_on_close = config.windows.focus_on_close.clone();
        self.cross_workspace_focus = config.input.cross_workspace_focus;
        // Reconfigure all windows to apply new gap values.
        let all_ids: Vec<WindowId> = self.windows.keys().copied().collect();
        for id in all_ids {
            self.send_configure(id);
        }
    }

    // ── Output management ────────────────────────────────────────────────────

    pub fn set_output_size(&mut self, w: i32, h: i32) {
        self.output_w = w;
        self.output_h = h;
        // Reset usable area to full output (will be adjusted by exclusive zones).
        self.usable_w = w;
        self.usable_h = h;
        self.usable_x = 0;
        self.usable_y = 0;
        let all_ids: Vec<WindowId> = self.windows.keys().copied().collect();
        for id in all_ids {
            self.send_configure(id);
        }
    }

    /// Set the usable area for tiled window layout (after exclusive zones).
    pub fn set_usable_area(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.usable_x = x;
        self.usable_y = y;
        self.usable_w = w;
        self.usable_h = h;
        let all_ids: Vec<WindowId> = self.windows.keys().copied().collect();
        for id in all_ids {
            self.send_configure(id);
        }
    }

    // ── Workspace accessors ──────────────────────────────────────────────────

    fn ws(&self, col: u8, row: u8) -> &Workspace {
        &self.workspaces[row as usize][col as usize]
    }

    fn ws_mut(&mut self, col: u8, row: u8) -> &mut Workspace {
        &mut self.workspaces[row as usize][col as usize]
    }

    pub fn focused_ws(&self) -> &Workspace {
        self.ws(self.focused.0, self.focused.1)
    }

    // ── Window placement ─────────────────────────────────────────────────────

    /// Register a newly-mapped xdg_toplevel, run placement policy, return id + adaptation info.
    ///
    /// `hints` allows window rules to pre-empt the normal placement policy.
    pub fn add_window(
        &mut self,
        surface: ToplevelSurface,
        hints: PlacementHints,
    ) -> AddWindowResult {
        let id = self.alloc_id();
        let surface_id = surface.wl_surface().id();
        let ws_coords = hints.workspace.unwrap_or(self.focused);

        // Determine initial placement and any demotion actions.
        // Rule hints override the policy's state/slot selection.
        let (initial, demotions) = {
            let ws = self.ws(ws_coords.0, ws_coords.1);
            let mut placement = placement::place_new_window(ws);
            if let Some(state) = hints.state {
                placement.0.state = state;
            }
            if hints.slot.is_some() {
                placement.0.slot = hints.slot;
            }
            placement
        };

        // Apply any required demotions (e.g. Fullscreen → HalfLeft).
        let mut demote_ids: Vec<WindowId> = Vec::new();
        for action in &demotions {
            {
                let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                ws.fullscreen_stack.retain(|&x| x != action.window_id);
                ws.total_fullscreen_stack.retain(|&x| x != action.window_id);
                ws.push_to_slot_top(action.to_slot, action.window_id);
            }
            if let Some(w) = self.windows.get_mut(&action.window_id) {
                w.current_state = action.new_state;
                w.current_slot = Some(action.to_slot);
            }
            demote_ids.push(action.window_id);
        }

        // Run conflict resolution for the incoming window.
        let resolved = {
            let ws = self.ws(ws_coords.0, ws_coords.1);
            conflict::resolve(
                ws,
                initial.state,
                initial.slot,
                None,
                &self.conflict_policy,
                self.slot_adaptation,
                self.fullscreen_adaptation,
            )
        };

        // Handle displaced windows (Swap policy).
        for displaced in &resolved.displaced {
            {
                let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                ws.slots
                    .entry(displaced.from_slot)
                    .or_default()
                    .retain(|&x| x != displaced.id);
                ws.add_floating(displaced.id);
            }
            if let Some(w) = self.windows.get_mut(&displaced.id) {
                w.current_state = WindowState::Floating;
                w.current_slot = None;
            }
            demote_ids.push(displaced.id);
        }

        // Compute the window's actual final state/slot from the resolution.
        // Also determine what (if any) adaptation occurred for IPC events (§6.5.1).
        let mut slot_adapted: Option<(Slot, Slot)> = None;
        let mut state_adapted: Option<(WindowState, WindowState, Option<Slot>)> = None;
        let (final_state, final_slot) = match &resolved.placement {
            conflict::Placement::Tiled { slot, adapted } => {
                if *adapted {
                    if initial.state != WindowState::Tiled {
                        // Fullscreen → Tiled adaptation (§6.5.1 fullscreen cascade).
                        state_adapted = Some((initial.state, WindowState::Tiled, Some(*slot)));
                    } else if let Some(req_slot) = initial.slot && req_slot != *slot {
                        // Half → Quarter slot adaptation (§6.5.1).
                        slot_adapted = Some((req_slot, *slot));
                    }
                }
                (WindowState::Tiled, Some(*slot))
            }
            conflict::Placement::FullscreenPromotion => (WindowState::Fullscreen, None),
            conflict::Placement::Floating => (WindowState::Floating, None),
        };

        // Floating geom: use rule hint if provided, else center 800×600.
        let floating_geom = hints.floating_geom.unwrap_or_else(|| {
            let float_w = 800.min(self.output_w);
            let float_h = 600.min(self.output_h);
            Rect::new(
                (self.output_w - float_w) / 2,
                (self.output_h - float_h) / 2,
                float_w,
                float_h,
            )
        });

        // Add to workspace data structures.
        {
            let ws = self.ws_mut(ws_coords.0, ws_coords.1);
            match final_state {
                WindowState::Tiled => {
                    if let Some(slot) = final_slot {
                        ws.push_to_slot_top(slot, id);
                    }
                }
                WindowState::Fullscreen => {
                    ws.fullscreen_stack.insert(0, id);
                }
                WindowState::TotalFullscreen => {
                    ws.total_fullscreen_stack.insert(0, id);
                }
                WindowState::Floating => {
                    ws.add_floating(id);
                }
            }
        }

        let window = Window {
            id,
            app_id: String::new(),
            title: String::new(),
            workspace: ws_coords,
            current_state: final_state,
            requested_state: initial.state,
            current_slot: final_slot,
            requested_slot: initial.slot,
            stack_index: 0,
            floating_geom,
            surface,
            surface_id: surface_id.clone(),
            is_urgent: false,
            pid: hints.pid,
            swallowed_id: None,
            is_swallowed: false,
            opacity: hints.opacity.unwrap_or(1.0),
            sticky: hints.pin || hints.sticky,
            no_animations: hints.no_animations,
            above_total_fullscreen: hints.above_total_fullscreen.unwrap_or(true),
            border_color: hints.border_color,
            shader: hints.shader,
            steal_focus: hints.steal_focus,
        };

        self.windows.insert(id, window);
        self.surface_to_window.insert(surface_id, id);

        // §22.4 window swallowing: hide the swallowed window and take its slot.
        if let Some(swallowed_id) = hints.swallows {
            // Mark swallowed window as hidden so the renderer skips it.
            if let Some(sw) = self.windows.get_mut(&swallowed_id) {
                sw.is_swallowed = true;
            }
            // Record the swallowing relationship on the new window.
            if let Some(w) = self.windows.get_mut(&id) {
                w.swallowed_id = Some(swallowed_id);
            }
        }

        // Update focus (skip if the rule said no_focus).
        self.ws_mut(ws_coords.0, ws_coords.1).record_focus(id);
        if !hints.no_focus {
            self.focused_window = Some(id);
        }

        // Send configure to newly-placed window and all demoted windows.
        let to_configure: Vec<WindowId> = std::iter::once(id).chain(demote_ids).collect();
        for wid in to_configure {
            self.send_configure(wid);
        }

        AddWindowResult {
            id,
            slot_adapted,
            state_adapted,
        }
    }

    /// Remove a window by its Wayland surface ID. Returns the WindowId if found.
    pub fn remove_window(&mut self, surface_id: &ObjectId) -> Option<WindowId> {
        let id = self.surface_to_window.remove(surface_id)?;
        let window = self.windows.remove(&id)?;
        let ws_coords = window.workspace;
        let closed_slot = window.current_slot;

        // Guard: special workspace windows use (MAX,MAX) and have no grid slot.
        if ws_coords.0 != u8::MAX && ws_coords.1 != u8::MAX {
            let ws = self.ws_mut(ws_coords.0, ws_coords.1);
            ws.remove_window(id);
        }

        // Focus-on-close policy.
        if self.focused_window == Some(id) {
            if ws_coords.0 != u8::MAX && ws_coords.1 != u8::MAX {
                let ws = self.ws(ws_coords.0, ws_coords.1);
                self.focused_window =
                    focus::focus_after_close(ws, id, closed_slot, &self.focus_on_close.clone());
            } else {
                self.focused_window = self
                    .ws(self.focused.0, self.focused.1)
                    .focus_history
                    .front()
                    .copied();
            }
        }

        // Auto-restore Fullscreen windows if conditions are now met.
        if self.fullscreen_auto_restore && ws_coords.0 != u8::MAX {
            self.check_auto_restore(ws_coords);
        }

        // §22.4 window swallowing: if this window swallowed another, unswallow it.
        if let Some(swallowed_id) = window.swallowed_id && let Some(sw) = self.windows.get_mut(&swallowed_id) {
            sw.is_swallowed = false;
            // Give focus back to the swallowed window.
            self.focused_window = Some(swallowed_id);
        }

        Some(id)
    }

    /// Move a window from its current workspace into a named special workspace.
    /// Sets `window.workspace` to the sentinel `(u8::MAX, u8::MAX)`.
    pub fn detach_window_to_special(&mut self, id: WindowId) {
        let Some(win) = self.windows.get(&id) else {
            return;
        };
        let old_ws = win.workspace;
        if old_ws.0 == u8::MAX {
            return; // already in a special workspace
        }
        // Remove from the grid workspace tracking.
        self.ws_mut(old_ws.0, old_ws.1).remove_window(id);
        if let Some(win) = self.windows.get_mut(&id) {
            win.workspace = (u8::MAX, u8::MAX);
        }
        // Restore focus to next window in the workspace.
        if self.focused_window == Some(id) {
            self.focused_window = self
                .ws(self.focused.0, self.focused.1)
                .focus_history
                .front()
                .copied();
        }
    }

    /// Return a window from a special workspace back into `target_ws`.
    /// Applies normal placement policy.
    pub fn attach_window_from_special(&mut self, id: WindowId, target_ws: (u8, u8)) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        if win.workspace != (u8::MAX, u8::MAX) {
            return; // not in a special workspace
        }
        win.workspace = target_ws;
        // Add to the target workspace as a floating window if no slot assigned.
        let ws = self.ws_mut(target_ws.0, target_ws.1);
        ws.add_floating(id);
        self.focused_window = Some(id);
    }

    // ── Auto-restore (§6.5.1) ────────────────────────────────────────────────

    /// Promote any window whose requested_state is Fullscreen but
    /// current_state is Tiled, if the workspace has cleared enough.
    fn check_auto_restore(&mut self, ws_coords: (u8, u8)) {
        let candidates: Vec<WindowId> = self
            .windows
            .values()
            .filter(|w| {
                w.workspace == ws_coords
                    && w.requested_state == WindowState::Fullscreen
                    && w.current_state == WindowState::Tiled
            })
            .map(|w| w.id)
            .collect();

        for wid in candidates {
            let can_promote = {
                let ws = self.ws(ws_coords.0, ws_coords.1);
                // Promote when this is the only tiled window left.
                ws.tiled_count() == 1
                    && ws
                        .slot_stack(self.windows[&wid].current_slot.unwrap_or(Slot::HalfLeft))
                        .iter()
                        .all(|&x| x == wid)
            };

            if can_promote {
                let old_slot = self.windows[&wid].current_slot;
                {
                    let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                    if let Some(slot) = old_slot {
                        ws.slots.entry(slot).or_default().retain(|&x| x != wid);
                    }
                    ws.fullscreen_stack.insert(0, wid);
                }
                if let Some(w) = self.windows.get_mut(&wid) {
                    w.current_state = WindowState::Fullscreen;
                    w.current_slot = None;
                }
                self.send_configure(wid);
                tracing::debug!(id = wid, "Auto-restored window to Fullscreen");
            }
        }
    }

    // ── Geometry ─────────────────────────────────────────────────────────────

    /// Look up a WindowId by the Wayland surface ObjectId (for transient detection).
    pub fn window_id_for_surface(&self, surface_id: &ObjectId) -> Option<WindowId> {
        self.surface_to_window.get(surface_id).copied()
    }

    /// Compute the pixel Rect for a window given its current state/slot.
    pub fn compute_rect(&self, id: WindowId) -> Option<Rect> {
        let window = self.windows.get(&id)?;
        // Special-workspace windows (sentinel (MAX,MAX)) have no layout position.
        if window.workspace.0 == u8::MAX {
            return None;
        }
        let ws = self.ws(window.workspace.0, window.workspace.1);
        let smart = ws.should_use_smart_gaps();

        Some(match window.current_state {
            WindowState::Tiled => {
                let slot = window.current_slot?;
                let mut r = layout::slot_rect(
                    slot,
                    self.usable_w,
                    self.usable_h,
                    self.inner_gap_px,
                    self.outer_gap_px,
                    smart,
                );
                // Offset by usable area origin (e.g. below a top bar).
                r.x += self.usable_x;
                r.y += self.usable_y;
                r
            }
            WindowState::Floating => window.floating_geom,
            WindowState::Fullscreen => {
                // Fullscreen respects exclusive zones (stays within usable area).
                Rect::new(
                    self.usable_x,
                    self.usable_y,
                    self.usable_w.max(1),
                    self.usable_h.max(1),
                )
            }
            WindowState::TotalFullscreen => {
                layout::total_fullscreen_rect(self.output_w, self.output_h)
            }
        })
    }

    /// Send a configure event to a window with its computed geometry.
    pub fn send_configure(&self, id: WindowId) {
        let Some(window) = self.windows.get(&id) else {
            return;
        };
        let Some(rect) = self.compute_rect(id) else {
            return;
        };

        let is_focused = self.focused_window == Some(id);
        window.surface.with_pending_state(|state| {
            state.size = Some((rect.w, rect.h).into());
            if is_focused {
                state.states.set(xdg_toplevel::State::Activated);
            } else {
                state.states.unset(xdg_toplevel::State::Activated);
            }
            match window.current_state {
                WindowState::Fullscreen | WindowState::TotalFullscreen => {
                    state.states.set(xdg_toplevel::State::Fullscreen);
                }
                _ => {
                    state.states.unset(xdg_toplevel::State::Fullscreen);
                }
            }
        });
        window.surface.send_configure();
    }

    // ── Rendering helpers ────────────────────────────────────────────────────

    /// Windows on the focused workspace in bottom→top Z order, paired with rects.
    pub fn visible_windows(&self) -> Vec<(&Window, Rect)> {
        self.visible_windows_for_ws(self.focused)
    }

    /// Windows on a specific workspace in bottom→top Z order, paired with rects.
    pub fn visible_windows_for_ws(&self, ws_coords: (u8, u8)) -> Vec<(&Window, Rect)> {
        if ws_coords.0 >= self.cols || ws_coords.1 >= self.rows {
            return vec![];
        }
        let ws = self.ws(ws_coords.0, ws_coords.1);
        ws.windows_in_z_order()
            .into_iter()
            .filter_map(|id| {
                let rect = self.compute_rect(id)?;
                let window = self.windows.get(&id)?;
                // Skip windows hidden by swallowing (§22.4).
                if window.is_swallowed {
                    return None;
                }
                Some((window, rect))
            })
            .collect()
    }

    /// Hit-test: return the topmost window at logical pixel `(x, y)` on the
    /// focused workspace (used for click-to-focus and drag detection).
    pub fn window_at(&self, x: f64, y: f64) -> Option<WindowId> {
        let ws_coords = self.focused;
        let ws = self.ws(ws_coords.0, ws_coords.1);
        // Walk Z-order top→bottom (reverse) so we return the topmost hit.
        for &id in ws.windows_in_z_order().iter().rev() {
            if let Some(rect) = self.compute_rect(id)
                && x >= rect.x as f64
                && x < (rect.x + rect.w) as f64
                && y >= rect.y as f64
                && y < (rect.y + rect.h) as f64
            {
                return Some(id);
            }
        }
        None
    }

    // ── Lookup ────────────────────────────────────────────────────────────────

    pub fn window_by_surface(&self, id: &ObjectId) -> Option<WindowId> {
        self.surface_to_window.get(id).copied()
    }

    // ── Focus management ─────────────────────────────────────────────────────

    /// Return the wl_surface of the currently focused window (for seat input).
    pub fn focused_surface(
        &self,
    ) -> Option<&smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        self.focused_window
            .and_then(|id| self.windows.get(&id))
            .map(|w| w.surface.wl_surface())
    }

    /// Set focus to `id`, updating history.
    pub fn set_focus(&mut self, id: WindowId) {
        let ws_coords = match self.windows.get(&id) {
            Some(w) => w.workspace,
            None => return,
        };
        self.focused_window = Some(id);
        if let Some(w) = self.windows.get_mut(&id) {
            w.is_urgent = false;
        }
        if ws_coords.0 != u8::MAX {
            self.ws_mut(ws_coords.0, ws_coords.1).record_focus(id);
        }
    }

    // ── Workspace navigation (§5) ────────────────────────────────────────────

    pub fn wrap_x(&self) -> bool {
        self.wrap_x
    }
    pub fn wrap_y(&self) -> bool {
        self.wrap_y
    }

    pub fn navigate(&mut self, direction: FocusDirection) {
        let (col, row) = self.focused;
        let new_coords = match direction {
            FocusDirection::Left => {
                if col > 0 {
                    (col - 1, row)
                } else if self.wrap_x {
                    (self.cols - 1, row)
                } else {
                    return;
                }
            }
            FocusDirection::Right => {
                if col + 1 < self.cols {
                    (col + 1, row)
                } else if self.wrap_x {
                    (0, row)
                } else {
                    return;
                }
            }
            FocusDirection::Up => {
                if row > 0 {
                    (col, row - 1)
                } else if self.wrap_y {
                    (col, self.rows - 1)
                } else {
                    return;
                }
            }
            FocusDirection::Down => {
                if row + 1 < self.rows {
                    (col, row + 1)
                } else if self.wrap_y {
                    (col, 0)
                } else {
                    return;
                }
            }
        };
        self.switch_workspace(new_coords);
    }

    pub fn switch_workspace(&mut self, coords: (u8, u8)) {
        if coords == self.focused {
            return;
        }
        let old = self.focused;
        self.history.push_front(old);
        if self.history.len() > 64 {
            self.history.pop_back();
        }
        // Manual switch clears the forward stack.
        self.forward_history.clear();
        self.focused = coords;
        // Restore focus to the last-focused window in the new workspace.
        self.focused_window = self.ws(coords.0, coords.1).focus_history.front().copied();
        tracing::debug!(col = coords.0, row = coords.1, "Switched workspace");
    }

    pub fn workspace_back(&mut self) {
        if let Some(prev) = self.history.pop_front() {
            let current = self.focused;
            self.forward_history.push_front(current);
            if self.forward_history.len() > 64 {
                self.forward_history.pop_back();
            }
            self.focused = prev;
            self.focused_window =
                self.ws(prev.0, prev.1).focus_history.front().copied();
        }
    }

    pub fn workspace_forward(&mut self) {
        if let Some(next) = self.forward_history.pop_front() {
            let current = self.focused;
            self.history.push_front(current);
            if self.history.len() > 64 {
                self.history.pop_back();
            }
            self.focused = next;
            self.focused_window = self.ws(next.0, next.1).focus_history.front().copied();
        }
    }

    // ── Window metadata (Phase 2f) ───────────────────────────────────────────

    pub fn update_window_metadata(&mut self, id: WindowId, app_id: String, title: String) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.app_id = app_id;
            w.title = title;
        }
    }

    // ── Close ─────────────────────────────────────────────────────────────────

    /// Send an xdg_toplevel.close request to the focused window.
    pub fn close_focused(&self) {
        if let Some(id) = self.focused_window && let Some(w) = self.windows.get(&id) {
            w.surface.send_close();
        }
    }

    // ── Spatial focus navigation (§6.11) ────────────────────────────────────

    /// Move keyboard focus in `direction` within or across workspaces.
    pub fn move_focus(&mut self, direction: FocusDirection) {
        let (col, row) = self.focused;

        let next = self.focused_window.and_then(|id| {
            let slot = self.windows.get(&id)?.current_slot?;
            focus::spatial_focus(self.ws(col, row), slot, direction)
        });

        if let Some(next_id) = next {
            self.focused_window = Some(next_id);
            self.ws_mut(col, row).record_focus(next_id);
        } else if self.cross_workspace_focus {
            self.navigate(direction);
        }
    }

    // ── State toggles ─────────────────────────────────────────────────────────

    /// Toggle the focused window between Fullscreen and Tiled (§6.3).
    pub fn toggle_fullscreen(&mut self) {
        let Some(id) = self.focused_window else {
            return;
        };
        let Some(w) = self.windows.get(&id) else {
            return;
        };
        let (ws_coords, current_state, current_slot) =
            (w.workspace, w.current_state, w.current_slot);

        match current_state {
            WindowState::Fullscreen => {
                // Demote: Fullscreen → Tiled in remembered/default slot
                let slot = current_slot.unwrap_or(Slot::HalfLeft);
                {
                    let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                    ws.fullscreen_stack.retain(|&x| x != id);
                    ws.push_to_slot_top(slot, id);
                }
                if let Some(w) = self.windows.get_mut(&id) {
                    w.current_state = WindowState::Tiled;
                    w.current_slot = Some(slot);
                    w.requested_state = WindowState::Tiled;
                }
            }
            WindowState::TotalFullscreen => {
                // Step down: TotalFullscreen → Fullscreen
                {
                    let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                    ws.total_fullscreen_stack.retain(|&x| x != id);
                    ws.fullscreen_stack.insert(0, id);
                }
                if let Some(w) = self.windows.get_mut(&id) {
                    w.current_state = WindowState::Fullscreen;
                }
            }
            WindowState::Tiled | WindowState::Floating => {
                // Promote: Tiled/Floating → Fullscreen
                self.detach_from_stacks(id, ws_coords);
                {
                    let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                    ws.fullscreen_stack.insert(0, id);
                }
                if let Some(w) = self.windows.get_mut(&id) {
                    w.current_state = WindowState::Fullscreen;
                    w.requested_state = WindowState::Fullscreen;
                }
            }
        }
        self.send_configure(id);
    }

    /// Toggle the focused window between TotalFullscreen and Tiled (§6.3).
    pub fn toggle_total_fullscreen(&mut self) {
        let Some(id) = self.focused_window else {
            return;
        };
        let Some(w) = self.windows.get(&id) else {
            return;
        };
        let (ws_coords, current_state, current_slot) =
            (w.workspace, w.current_state, w.current_slot);

        match current_state {
            WindowState::TotalFullscreen => {
                // Demote → Tiled
                let slot = current_slot.unwrap_or(Slot::HalfLeft);
                {
                    let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                    ws.total_fullscreen_stack.retain(|&x| x != id);
                    ws.push_to_slot_top(slot, id);
                }
                if let Some(w) = self.windows.get_mut(&id) {
                    w.current_state = WindowState::Tiled;
                    w.current_slot = Some(slot);
                    w.requested_state = WindowState::Tiled;
                }
            }
            _ => {
                // Promote → TotalFullscreen
                self.detach_from_stacks(id, ws_coords);
                {
                    let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                    ws.total_fullscreen_stack.insert(0, id);
                }
                if let Some(w) = self.windows.get_mut(&id) {
                    w.current_state = WindowState::TotalFullscreen;
                    w.requested_state = WindowState::TotalFullscreen;
                }
            }
        }
        self.send_configure(id);
    }

    /// Toggle the focused window between Floating and Tiled (§6.3).
    pub fn toggle_floating(&mut self) {
        let Some(id) = self.focused_window else {
            return;
        };
        let Some(w) = self.windows.get(&id) else {
            return;
        };
        let (ws_coords, current_state) = (w.workspace, w.current_state);

        match current_state {
            WindowState::Floating => {
                // Demote: Floating → Tiled (restore requested slot or default)
                let slot = self.windows[&id].requested_slot.unwrap_or(Slot::HalfLeft);
                {
                    let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                    ws.floating.retain(|&x| x != id);
                    ws.push_to_slot_top(slot, id);
                }
                if let Some(w) = self.windows.get_mut(&id) {
                    w.current_state = WindowState::Tiled;
                    w.current_slot = Some(slot);
                    w.requested_state = WindowState::Tiled;
                }
            }
            _ => {
                // Promote: anything → Floating
                self.detach_from_stacks(id, ws_coords);
                {
                    let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                    ws.add_floating(id);
                }
                if let Some(w) = self.windows.get_mut(&id) {
                    w.current_state = WindowState::Floating;
                    w.current_slot = None;
                    w.requested_state = WindowState::Floating;
                }
            }
        }
        self.send_configure(id);
    }

    // ── Slot assignment ───────────────────────────────────────────────────────

    /// Move the focused window into `slot`, running conflict resolution (§6.5).
    pub fn assign_slot(&mut self, slot: Slot) {
        let Some(id) = self.focused_window else {
            return;
        };
        let ws_coords = match self.windows.get(&id) {
            Some(w) => w.workspace,
            None => return,
        };

        // Detach from current position (preserving focus history).
        self.detach_from_stacks(id, ws_coords);

        // Run conflict resolution against the target slot.
        let policy = self.conflict_policy.clone();
        let slot_adaptation = self.slot_adaptation;
        let resolved = {
            let ws = self.ws(ws_coords.0, ws_coords.1);
            conflict::resolve(
                ws,
                WindowState::Tiled,
                Some(slot),
                None,
                &policy,
                slot_adaptation,
                false,
            )
        };

        // Displace any kicked-out windows to floating.
        let displaced: Vec<_> = resolved
            .displaced
            .iter()
            .map(|d| (d.id, d.from_slot))
            .collect();
        for (disp_id, from_slot) in displaced {
            {
                let ws = self.ws_mut(ws_coords.0, ws_coords.1);
                ws.slots
                    .entry(from_slot)
                    .or_default()
                    .retain(|&x| x != disp_id);
                ws.add_floating(disp_id);
            }
            if let Some(w) = self.windows.get_mut(&disp_id) {
                w.current_state = WindowState::Floating;
                w.current_slot = None;
            }
            self.send_configure(disp_id);
        }

        let (final_state, final_slot) = match &resolved.placement {
            conflict::Placement::Tiled { slot: s, .. } => (WindowState::Tiled, Some(*s)),
            conflict::Placement::Floating => (WindowState::Floating, None),
            conflict::Placement::FullscreenPromotion => (WindowState::Fullscreen, None),
        };

        {
            let ws = self.ws_mut(ws_coords.0, ws_coords.1);
            match final_state {
                WindowState::Tiled => {
                    if let Some(s) = final_slot {
                        ws.push_to_slot_top(s, id);
                    }
                }
                WindowState::Floating => ws.add_floating(id),
                WindowState::Fullscreen => ws.fullscreen_stack.insert(0, id),
                WindowState::TotalFullscreen => ws.total_fullscreen_stack.insert(0, id),
            }
        }

        if let Some(w) = self.windows.get_mut(&id) {
            w.current_state = final_state;
            w.requested_state = WindowState::Tiled;
            w.requested_slot = Some(slot);
            w.current_slot = final_slot;
        }
        self.focused_window = Some(id);
        self.send_configure(id);
    }

    // ── Stack cycling (§6.8) ─────────────────────────────────────────────────

    /// Jump directly to stack index `n` (0 = top). No-op if out of bounds.
    pub fn stack_flip(&mut self, n: u32) {
        let Some(id) = self.focused_window else {
            return;
        };
        let Some(w) = self.windows.get(&id) else {
            return;
        };
        let (ws_coords, slot) = (
            w.workspace,
            match w.current_slot {
                Some(s) => s,
                None => return,
            },
        );

        let new_top = {
            let stack = self.ws_mut(ws_coords.0, ws_coords.1).slot_stack_mut(slot);
            let n = n as usize;
            if n >= stack.len() {
                return;
            }
            let target = stack.remove(n);
            stack.insert(0, target);
            stack[0]
        };
        self.focused_window = Some(new_top);
        self.ws_mut(ws_coords.0, ws_coords.1).record_focus(new_top);
    }

    /// Rotate the focused slot's stack: make the next window the top (visible).
    pub fn stack_next(&mut self) {
        let Some(id) = self.focused_window else {
            return;
        };
        let Some(w) = self.windows.get(&id) else {
            return;
        };
        let (ws_coords, slot) = (
            w.workspace,
            match w.current_slot {
                Some(s) => s,
                None => return,
            },
        );

        let new_top = {
            let stack = self.ws_mut(ws_coords.0, ws_coords.1).slot_stack_mut(slot);
            if stack.len() < 2 {
                return;
            }
            let first = stack.remove(0);
            stack.push(first);
            stack[0]
        };
        self.focused_window = Some(new_top);
        self.ws_mut(ws_coords.0, ws_coords.1).record_focus(new_top);
    }

    /// Rotate the focused slot's stack: make the previous window the top.
    pub fn stack_prev(&mut self) {
        let Some(id) = self.focused_window else {
            return;
        };
        let Some(w) = self.windows.get(&id) else {
            return;
        };
        let (ws_coords, slot) = (
            w.workspace,
            match w.current_slot {
                Some(s) => s,
                None => return,
            },
        );

        let new_top = {
            let stack = self.ws_mut(ws_coords.0, ws_coords.1).slot_stack_mut(slot);
            if stack.len() < 2 {
                return;
            }
            let last = stack.pop().unwrap();
            stack.insert(0, last);
            stack[0]
        };
        self.focused_window = Some(new_top);
        self.ws_mut(ws_coords.0, ws_coords.1).record_focus(new_top);
    }

    /// Move the focused window to position 0 of its slot's stack.
    /// No-op when the window is already the top (which is the invariant maintained
    /// by `stack_next`/`stack_prev`), but useful for future non-cycling focus modes.
    pub fn stack_promote(&mut self) {
        let Some(id) = self.focused_window else {
            return;
        };
        let Some(w) = self.windows.get(&id) else {
            return;
        };
        let (ws_coords, slot) = (
            w.workspace,
            match w.current_slot {
                Some(s) => s,
                None => return,
            },
        );
        let stack = self.ws_mut(ws_coords.0, ws_coords.1).slot_stack_mut(slot);
        if let Some(pos) = stack.iter().position(|&x| x == id) && pos > 0 {
            stack.remove(pos);
            stack.insert(0, id);
        }
        self.focused_window = Some(id);
        self.ws_mut(ws_coords.0, ws_coords.1).record_focus(id);
    }

    /// Eject the focused window from its tiled slot into the Floating layer.
    pub fn stack_collapse(&mut self) {
        let Some(id) = self.focused_window else {
            return;
        };
        let (ws_coords, slot) = {
            let Some(w) = self.windows.get(&id) else {
                return;
            };
            if w.current_state != WindowState::Tiled {
                return;
            }
            (
                w.workspace,
                match w.current_slot {
                    Some(s) => s,
                    None => return,
                },
            )
        };
        // Remove from slot stack.
        {
            let stack = self.ws_mut(ws_coords.0, ws_coords.1).slot_stack_mut(slot);
            stack.retain(|&x| x != id);
        }
        // Add to floating layer.
        self.ws_mut(ws_coords.0, ws_coords.1).floating.push(id);
        // Centre the window on screen as initial floating geometry.
        let (ow, oh) = (self.output_w, self.output_h);
        if let Some(w) = self.windows.get_mut(&id) {
            let fw = w.floating_geom.w.max(400);
            let fh = w.floating_geom.h.max(300);
            w.floating_geom = Rect {
                x: (ow - fw) / 2,
                y: (oh - fh) / 2,
                w: fw,
                h: fh,
            };
            w.current_state = WindowState::Floating;
            w.current_slot = None;
        }
        self.send_configure(id);
        tracing::debug!(id, "Ejected window from stack to floating");
    }

    /// Move the focused window one position toward index 0 in its slot's stack.
    pub fn stack_move_up(&mut self) {
        let Some(id) = self.focused_window else {
            return;
        };
        let Some(w) = self.windows.get(&id) else {
            return;
        };
        let (ws_coords, slot) = (
            w.workspace,
            match w.current_slot {
                Some(s) => s,
                None => return,
            },
        );
        let stack = self.ws_mut(ws_coords.0, ws_coords.1).slot_stack_mut(slot);
        if let Some(pos) = stack.iter().position(|&x| x == id) && pos > 0 {
            stack.swap(pos, pos - 1);
            // Re-focus the top of the stack.
            self.focused_window = Some(stack[0]);
        }
    }

    /// Move the focused window one position toward the end of its slot's stack.
    /// The window below it becomes the new visible (index 0) window.
    pub fn stack_move_down(&mut self) {
        let Some(id) = self.focused_window else {
            return;
        };
        let Some(w) = self.windows.get(&id) else {
            return;
        };
        let (ws_coords, slot) = (
            w.workspace,
            match w.current_slot {
                Some(s) => s,
                None => return,
            },
        );
        let stack = self.ws_mut(ws_coords.0, ws_coords.1).slot_stack_mut(slot);
        if let Some(pos) = stack.iter().position(|&x| x == id) && pos + 1 < stack.len() {
            stack.swap(pos, pos + 1);
            // The window that surfaced to index 0 is now the visible/focused one.
            let new_top = stack[0];
            self.focused_window = Some(new_top);
            self.ws_mut(ws_coords.0, ws_coords.1).record_focus(new_top);
        }
    }

    /// Rename a workspace cell. Stores the name for IPC/OSD display.
    pub fn rename_workspace(&mut self, col: u8, row: u8, name: String) {
        if col < self.cols && row < self.rows {
            self.ws_mut(col, row).name = if name.is_empty() { None } else { Some(name) };
        }
    }

    /// Return the current name of a workspace cell (empty string if unnamed).
    pub fn workspace_name(&self, col: u8, row: u8) -> &str {
        if col < self.cols && row < self.rows {
            self.ws(col, row).name.as_deref().unwrap_or("")
        } else {
            ""
        }
    }

    /// Return the number of windows in the given slot stack on workspace `ws`.
    pub fn slot_stack_size(&self, ws: (u8, u8), slot: Slot) -> usize {
        if ws.0 >= self.cols || ws.1 >= self.rows {
            return 0;
        }
        self.ws(ws.0, ws.1)
            .slots
            .get(&slot)
            .map(|s| s.len())
            .unwrap_or(0)
    }

    /// Return all window IDs in the slot stack (index 0 = top/visible) for IPC reorder events.
    pub fn slot_stack_ids(&self, ws: (u8, u8), slot: Slot) -> Vec<WindowId> {
        if ws.0 >= self.cols || ws.1 >= self.rows {
            return vec![];
        }
        self.ws(ws.0, ws.1)
            .slots
            .get(&slot)
            .cloned()
            .unwrap_or_default()
    }

    /// Resize the workspace grid to `new_cols` × `new_rows`.
    ///
    /// New cells are added as empty workspaces. Windows in cells that fall
    /// outside the new bounds are moved to the nearest valid cell. The focused
    /// workspace is clamped to stay inside the new bounds.
    /// Returns the (old_cols, old_rows) for callers that need to emit events.
    pub fn resize_grid(&mut self, new_cols: u8, new_rows: u8) -> (u8, u8) {
        let new_cols = new_cols.clamp(1, 16);
        let new_rows = new_rows.clamp(1, 16);
        let old_cols = self.cols;
        let old_rows = self.rows;

        // Grow rows first (add new rows of workspaces).
        while (self.workspaces.len() as u8) < new_rows {
            let r = self.workspaces.len() as u8;
            let row: Vec<workspace::Workspace> = (0..old_cols.max(new_cols))
                .map(|c| workspace::Workspace::new(c, r))
                .collect();
            self.workspaces.push(row);
        }
        // Grow or shrink each row's columns.
        for (r, row) in self.workspaces.iter_mut().enumerate() {
            while (row.len() as u8) < new_cols {
                let c = row.len() as u8;
                row.push(workspace::Workspace::new(c, r as u8));
            }
        }

        self.cols = new_cols;
        self.rows = new_rows;

        // Move windows from out-of-bounds cells into the nearest valid cell.
        // Collect (id, old_ws, new_ws) for windows that need relocation.
        type WindowRelocation = (WindowId, (u8, u8), (u8, u8));
        let relocations: Vec<WindowRelocation> = self.windows.values()
            .filter_map(|w| {
                let (wc, wr) = w.workspace;
                if wc == u8::MAX { return None; } // special workspace
                let cc = wc.min(new_cols - 1);
                let cr = wr.min(new_rows - 1);
                if (cc, cr) != (wc, wr) { Some((w.id, (wc, wr), (cc, cr))) }
                else { None }
            })
            .collect();
        for (id, old_ws, new_ws) in relocations {
            // Remove from old workspace tracking.
            self.workspaces[old_ws.1 as usize][old_ws.0 as usize].remove_window(id);
            // Add to new workspace — floating is the safest state after relocation.
            self.workspaces[new_ws.1 as usize][new_ws.0 as usize].add_floating(id);
            if let Some(w) = self.windows.get_mut(&id) {
                w.workspace = new_ws;
                w.current_state = WindowState::Floating;
                if w.current_slot.is_none() {
                    // Ensure the window has a valid floating geometry if it had none.
                    if w.floating_geom.w == 0 {
                        w.floating_geom = Rect::new(100, 100, 800, 600);
                    }
                }
                w.current_slot = None;
            }
        }

        // Clamp focused workspace.
        self.focused.0 = self.focused.0.min(new_cols - 1);
        self.focused.1 = self.focused.1.min(new_rows - 1);

        (old_cols, old_rows)
    }

    // ── Move window between workspaces (§6.7) ────────────────────────────────

    /// Returns true if the workspace at `coords` has an active TotalFullscreen window (§6.7.1).
    pub fn workspace_has_total_fullscreen(&self, coords: (u8, u8)) -> bool {
        if coords.0 >= self.cols || coords.1 >= self.rows {
            return false;
        }
        !self
            .ws(coords.0, coords.1)
            .total_fullscreen_stack
            .is_empty()
    }

    /// Move the focused window one step in `direction`, skipping TotalFullscreen-protected
    /// workspaces for Tiled/Fullscreen movers (§6.7.1).
    ///
    /// Returns the move result so the caller can emit IPC events.
    pub fn move_window_direction_ex(&mut self, direction: FocusDirection) -> Option<MoveDirectionResult> {
        let id = self.focused_window?;
        let (ws_col, ws_row, mover_state) = match self.windows.get(&id) {
            Some(w) => (w.workspace.0, w.workspace.1, w.current_state),
            None => return None,
        };

        // Floating and TotalFullscreen movers are always allowed (§6.7.1).
        let tf_protected = matches!(mover_state, WindowState::Tiled | WindowState::Fullscreen);

        let step_fn = |col: u8, row: u8| -> Option<(u8, u8)> {
            match direction {
                FocusDirection::Left => {
                    if col > 0 {
                        Some((col - 1, row))
                    } else if self.wrap_x {
                        Some((self.cols - 1, row))
                    } else {
                        None
                    }
                }
                FocusDirection::Right => {
                    if col + 1 < self.cols {
                        Some((col + 1, row))
                    } else if self.wrap_x {
                        Some((0, row))
                    } else {
                        None
                    }
                }
                FocusDirection::Up => {
                    if row > 0 {
                        Some((col, row - 1))
                    } else if self.wrap_y {
                        Some((col, self.rows - 1))
                    } else {
                        None
                    }
                }
                FocusDirection::Down => {
                    if row + 1 < self.rows {
                        Some((col, row + 1))
                    } else if self.wrap_y {
                        Some((col, 0))
                    } else {
                        None
                    }
                }
            }
        };

        let first_target = step_fn(ws_col, ws_row)?;

        if !tf_protected || !self.workspace_has_total_fullscreen(first_target) {
            // Fast path: no TF protection issue.
            self.move_window_to(first_target);
            return Some(MoveDirectionResult::MovedDirect {
                target: first_target,
            });
        }

        // Walk past protected workspaces.
        let mut cur = first_target;
        let mut any_skipped = false;
        loop {
            if !self.workspace_has_total_fullscreen(cur) {
                self.move_window_to(cur);
                return Some(MoveDirectionResult::Moved {
                    requested: first_target,
                    actual: cur,
                    skipped: any_skipped,
                });
            }
            any_skipped = true;
            match step_fn(cur.0, cur.1) {
                Some(next) if next != first_target => cur = next,
                _ => return Some(MoveDirectionResult::Refused),
            }
        }
    }

    /// Move the focused window one step in `direction` (legacy no-return form).
    pub fn move_window_direction(&mut self, direction: FocusDirection) {
        self.move_window_direction_ex(direction);
    }

    /// Move the focused window to a specific workspace cell.
    pub fn move_window_to(&mut self, target: (u8, u8)) {
        let Some(id) = self.focused_window else {
            return;
        };
        let ws_coords = match self.windows.get(&id) {
            Some(w) => w.workspace,
            None => return,
        };
        if ws_coords == target {
            return;
        }
        if target.0 >= self.cols || target.1 >= self.rows {
            return;
        }

        let current_state = self.windows[&id].current_state;
        let current_slot = self.windows[&id].current_slot;

        // Detach from source workspace.
        self.detach_from_stacks(id, ws_coords);
        // Update workspace assignment.
        if let Some(w) = self.windows.get_mut(&id) {
            w.workspace = target;
        }

        // Place in the target workspace.
        let policy = self.conflict_policy.clone();
        let slot_adaptation = self.slot_adaptation;
        let fullscreen_adaptation = self.fullscreen_adaptation;
        let resolved = {
            let ws = self.ws(target.0, target.1);
            conflict::resolve(
                ws,
                current_state,
                current_slot,
                None,
                &policy,
                slot_adaptation,
                fullscreen_adaptation,
            )
        };

        let (final_state, final_slot) = match &resolved.placement {
            conflict::Placement::Tiled { slot, .. } => (WindowState::Tiled, Some(*slot)),
            conflict::Placement::FullscreenPromotion => (WindowState::Fullscreen, None),
            conflict::Placement::Floating => (WindowState::Floating, None),
        };
        {
            let ws = self.ws_mut(target.0, target.1);
            match final_state {
                WindowState::Tiled => {
                    if let Some(s) = final_slot {
                        ws.push_to_slot_top(s, id);
                    }
                }
                WindowState::Fullscreen => ws.fullscreen_stack.insert(0, id),
                WindowState::TotalFullscreen => ws.total_fullscreen_stack.insert(0, id),
                WindowState::Floating => ws.add_floating(id),
            }
        }
        if let Some(w) = self.windows.get_mut(&id) {
            w.current_state = final_state;
            w.current_slot = final_slot;
        }
        self.send_configure(id);
    }

    // ── §6.13 Min-size overflow ───────────────────────────────────────────────

    /// Force a tiled window to Floating because its slot is smaller than its
    /// declared min_size (§6.13 `min_size_overflow = "float"`).
    ///
    /// `min_w` / `min_h` are the client's minimum hints (0 = unconstrained on
    /// that axis).  The window is centred inside the slot area at those
    /// dimensions.  Returns `true` if the conversion was performed.
    pub fn force_min_size_float(&mut self, id: WindowId, min_w: i32, min_h: i32) -> bool {
        let (ws_coords, current_slot) = match self.windows.get(&id) {
            Some(w) if w.current_state == WindowState::Tiled => (w.workspace, w.current_slot),
            _ => return false,
        };

        let slot_rect = match self.compute_rect(id) {
            Some(r) => r,
            None => return false,
        };

        let too_narrow = min_w > 0 && slot_rect.w < min_w;
        let too_short = min_h > 0 && slot_rect.h < min_h;
        if !too_narrow && !too_short {
            return false;
        }

        let float_w = if min_w > 0 { min_w } else { slot_rect.w };
        let float_h = if min_h > 0 { min_h } else { slot_rect.h };
        // Centre inside the slot area.
        let float_x = slot_rect.x + (slot_rect.w - float_w) / 2;
        let float_y = slot_rect.y + (slot_rect.h - float_h) / 2;

        // Detach from slot stacks.
        self.detach_from_stacks(id, ws_coords);
        self.ws_mut(ws_coords.0, ws_coords.1).add_floating(id);

        if let Some(w) = self.windows.get_mut(&id) {
            w.current_state = WindowState::Floating;
            w.current_slot = None;
            // Keep requested_slot so restoring later is possible.
            if w.requested_slot.is_none() {
                w.requested_slot = current_slot;
            }
            w.floating_geom = Rect {
                x: float_x,
                y: float_y,
                w: float_w,
                h: float_h,
            };
        }

        self.send_configure(id);
        tracing::debug!(
            id,
            min_w,
            min_h,
            slot_w = slot_rect.w,
            slot_h = slot_rect.h,
            "min_size overflow: window forced to floating"
        );
        true
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn alloc_id(&mut self) -> WindowId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Remove a window from all slot/floating/promotion stacks WITHOUT touching
    /// focus history (used for reassignment, not removal from compositor).
    fn detach_from_stacks(&mut self, id: WindowId, ws_coords: (u8, u8)) {
        let ws = self.ws_mut(ws_coords.0, ws_coords.1);
        for stack in ws.slots.values_mut() {
            stack.retain(|&w| w != id);
        }
        ws.floating.retain(|&w| w != id);
        ws.fullscreen_stack.retain(|&w| w != id);
        ws.total_fullscreen_stack.retain(|&w| w != id);
        for slot in window::Slot::ALL {
            ws.reindex_slot(slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::Config;

    fn make_grid(cols: u8, rows: u8) -> Grid {
        let mut cfg = Config::default();
        cfg.grid.cols = cols;
        cfg.grid.rows = rows;
        Grid::new(&cfg, 1920, 1080)
    }

    #[test]
    fn resize_grid_grows() {
        let mut g = make_grid(3, 3);
        assert_eq!((g.cols, g.rows), (3, 3));
        let (old_c, old_r) = g.resize_grid(4, 4);
        assert_eq!((old_c, old_r), (3, 3));
        assert_eq!((g.cols, g.rows), (4, 4));
    }

    #[test]
    fn resize_grid_shrinks_and_clamps_focus() {
        let mut g = make_grid(3, 3);
        g.focused = (2, 2);
        g.resize_grid(2, 2);
        assert_eq!((g.cols, g.rows), (2, 2));
        assert_eq!(g.focused, (1, 1)); // clamped from (2,2) to (1,1)
    }

    #[test]
    fn resize_grid_min_1x1() {
        let mut g = make_grid(2, 2);
        g.resize_grid(0, 0);
        assert_eq!((g.cols, g.rows), (1, 1)); // clamped to minimum
    }

    #[test]
    fn slot_stack_ids_empty_slot() {
        let g = make_grid(3, 3);
        let ids = g.slot_stack_ids((0, 0), Slot::HalfLeft);
        assert!(ids.is_empty());
    }

    #[test]
    fn workspace_has_total_fullscreen_false_for_empty() {
        let g = make_grid(3, 3);
        assert!(!g.workspace_has_total_fullscreen((0, 0)));
        assert!(!g.workspace_has_total_fullscreen((99, 99))); // OOB
    }

    #[test]
    fn navigate_stops_at_boundary_without_wrap() {
        let mut g = make_grid(3, 3);
        g.focused = (0, 0);
        g.navigate(FocusDirection::Left);
        assert_eq!(
            g.focused,
            (0, 0),
            "should not move left past col=0 without wrap"
        );
    }

    #[test]
    fn navigate_wraps_horizontally_when_enabled() {
        let mut cfg = Config::default();
        cfg.grid.cols = 3;
        cfg.grid.rows = 3;
        cfg.grid.wrap_x = true;
        let mut g = Grid::new(&cfg, 1920, 1080);
        g.focused = (0, 1);
        g.navigate(FocusDirection::Left);
        assert_eq!(g.focused.0, 2, "should wrap left to last column");
    }

    #[test]
    fn navigate_wraps_vertically_when_enabled() {
        let mut cfg = Config::default();
        cfg.grid.cols = 3;
        cfg.grid.rows = 3;
        cfg.grid.wrap_y = true;
        let mut g = Grid::new(&cfg, 1920, 1080);
        g.focused = (1, 0);
        g.navigate(FocusDirection::Up);
        assert_eq!(g.focused.1, 2, "should wrap up to last row");
    }

    #[test]
    fn grid_resize_preserves_focus_within_bounds() {
        let mut g = make_grid(3, 3);
        g.focused = (1, 1);
        g.resize_grid(4, 4);
        assert_eq!(
            g.focused,
            (1, 1),
            "focus should be preserved when still in bounds"
        );
    }

    #[test]
    fn navigate_right_from_last_col_stops_without_wrap() {
        let mut g = make_grid(3, 3);
        g.focused = (2, 0);
        g.navigate(FocusDirection::Right);
        assert_eq!(
            g.focused,
            (2, 0),
            "should not move right past last column without wrap"
        );
    }
}
