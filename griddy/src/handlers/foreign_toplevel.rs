//! wlr-foreign-toplevel-management-unstable-v1 (Phase 6).
//!
//! Allows shell clients (waybar, taskbars) to list open windows and optionally
//! manage them (activate, close, fullscreen toggle).

use std::collections::HashMap;

use smithay::utils::Serial;
use wayland_protocols_wlr::foreign_toplevel::v1::server::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use crate::grid::window::{WindowId, WindowState};
use crate::state::GlobalState;

// ─── State ───────────────────────────────────────────────────────────────────

pub struct WlrForeignToplevelState {
    pub managers: Vec<ZwlrForeignToplevelManagerV1>,
    /// Per-window handle resources sent to all bound managers.
    pub handles: HashMap<WindowId, Vec<ZwlrForeignToplevelHandleV1>>,
}

impl WlrForeignToplevelState {
    pub fn new() -> Self {
        Self { managers: Vec::new(), handles: HashMap::new() }
    }
}

// ─── State encoding ──────────────────────────────────────────────────────────

/// Encode window state as a wl_array of uint32 values (little-endian).
/// activated = 4, fullscreen = 8 (per wlr-foreign-toplevel protocol).
fn encode_wlr_state(window_state: WindowState, is_focused: bool) -> Vec<u8> {
    let mut states: Vec<u32> = Vec::new();
    if is_focused {
        states.push(4u32); // activated
    }
    if matches!(window_state, WindowState::Fullscreen | WindowState::TotalFullscreen) {
        states.push(8u32); // fullscreen
    }
    let mut bytes = Vec::with_capacity(states.len() * 4);
    for s in states {
        bytes.extend_from_slice(&s.to_ne_bytes());
    }
    bytes
}

// ─── Helper methods on GlobalState ───────────────────────────────────────────

impl GlobalState {
    /// Create wlr-foreign-toplevel handles for a newly mapped window on all
    /// currently bound manager clients.
    pub fn wlr_new_toplevel(&mut self, id: WindowId, title: String, app_id: String) {
        let Some(window) = self.grid.windows.get(&id) else { return };
        let window_state = window.current_state;
        let is_focused = self.grid.focused_window == Some(id);
        let state_bytes = encode_wlr_state(window_state, is_focused);
        let _ = window;

        let dh = self.display_handle.clone();
        let managers: Vec<ZwlrForeignToplevelManagerV1> = self
            .wlr_toplevel_state
            .managers
            .iter()
            .filter(|m| m.is_alive())
            .cloned()
            .collect();

        for manager in &managers {
            let Some(client) = manager.client() else { continue };
            let version = manager.version().min(3);
            let Ok(handle) =
                client.create_resource::<ZwlrForeignToplevelHandleV1, WindowId, GlobalState>(
                    &dh, version, id,
                )
            else {
                continue;
            };
            manager.toplevel(&handle);
            handle.title(title.clone());
            handle.app_id(app_id.clone());
            handle.state(state_bytes.clone());
            handle.done();
            self.wlr_toplevel_state.handles.entry(id).or_default().push(handle);
        }
    }

    /// Send `closed` on all handles for the given window and remove tracking.
    pub fn wlr_remove_toplevel(&mut self, id: WindowId) {
        if let Some(handles) = self.wlr_toplevel_state.handles.remove(&id) {
            for h in handles {
                if h.is_alive() {
                    h.closed();
                }
            }
        }
    }

    /// Send updated title to all handles for a window.
    pub fn wlr_update_title(&mut self, id: WindowId, title: &str) {
        if let Some(handles) = self.wlr_toplevel_state.handles.get(&id) {
            for h in handles {
                if h.is_alive() {
                    h.title(title.to_owned());
                    h.done();
                }
            }
        }
    }

    /// Send updated app_id to all handles for a window.
    pub fn wlr_update_app_id(&mut self, id: WindowId, app_id: &str) {
        if let Some(handles) = self.wlr_toplevel_state.handles.get(&id) {
            for h in handles {
                if h.is_alive() {
                    h.app_id(app_id.to_owned());
                    h.done();
                }
            }
        }
    }

    /// Send updated state (focus + window state) to all handles for a window.
    pub fn wlr_update_state(&mut self, id: WindowId) {
        let Some(window_state) = self.grid.windows.get(&id).map(|w| w.current_state) else {
            return;
        };
        let is_focused = self.grid.focused_window == Some(id);
        let state_bytes = encode_wlr_state(window_state, is_focused);
        if let Some(handles) = self.wlr_toplevel_state.handles.get(&id) {
            for h in handles {
                if h.is_alive() {
                    h.state(state_bytes.clone());
                    h.done();
                }
            }
        }
    }

    /// Refresh state for every tracked window — call after focus or workspace changes.
    pub fn wlr_update_all_states(&mut self) {
        let ids: Vec<WindowId> = self.wlr_toplevel_state.handles.keys().copied().collect();
        for id in ids {
            self.wlr_update_state(id);
        }
    }
}

// ─── GlobalDispatch — initial bind ───────────────────────────────────────────

impl GlobalDispatch<ZwlrForeignToplevelManagerV1, ()> for GlobalState {
    fn bind(
        state: &mut Self,
        dh: &DisplayHandle,
        client: &Client,
        resource: New<ZwlrForeignToplevelManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let manager = data_init.init(resource, ());

        // Snapshot existing windows (releases immutable borrow before we store handles).
        let windows: Vec<(WindowId, WindowState, bool, String, String)> = state
            .grid
            .windows
            .iter()
            .map(|(&wid, w)| {
                let is_focused = state.grid.focused_window == Some(wid);
                (wid, w.current_state, is_focused, w.title.clone(), w.app_id.clone())
            })
            .collect();

        for (window_id, window_state, is_focused, title, app_id) in windows {
            let version = manager.version().min(3);
            let Ok(handle) =
                client.create_resource::<ZwlrForeignToplevelHandleV1, WindowId, GlobalState>(
                    dh, version, window_id,
                )
            else {
                continue;
            };
            manager.toplevel(&handle);
            handle.title(title);
            handle.app_id(app_id);
            handle.state(encode_wlr_state(window_state, is_focused));
            handle.done();
            state.wlr_toplevel_state.handles.entry(window_id).or_default().push(handle);
        }

        state.wlr_toplevel_state.managers.push(manager);
    }
}

// ─── Dispatch — manager requests ─────────────────────────────────────────────

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for GlobalState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZwlrForeignToplevelManagerV1,
        request: zwlr_foreign_toplevel_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Request::Stop = request {
            state.wlr_toplevel_state.managers.retain(|m| m != resource);
            resource.finished();
        }
    }

    fn destroyed(
        state: &mut Self,
        _client_id: wayland_server::backend::ClientId,
        resource: &ZwlrForeignToplevelManagerV1,
        _data: &(),
    ) {
        state.wlr_toplevel_state.managers.retain(|m| m != resource);
    }
}

// ─── Dispatch — handle requests ──────────────────────────────────────────────

impl Dispatch<ZwlrForeignToplevelHandleV1, WindowId> for GlobalState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZwlrForeignToplevelHandleV1,
        request: zwlr_foreign_toplevel_handle_v1::Request,
        window_id: &WindowId,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        let id = *window_id;
        match request {
            zwlr_foreign_toplevel_handle_v1::Request::Activate { seat: _ } => {
                let ws = state.grid.windows.get(&id).map(|w| w.workspace);
                if let Some(coords) = ws {
                    if coords != state.grid.focused {
                        state.grid.switch_workspace(coords);
                        state.pending_events.push(crate::ipc::events::Event::WorkspaceChanged {
                            col: coords.0,
                            row: coords.1,
                        });
                    }
                }
                state.grid.set_focus(id);
                let surf = state.grid.focused_surface().cloned();
                if let (Some(surf), Some(kb)) = (surf, state.seat.get_keyboard()) {
                    let kb = kb.clone();
                    kb.set_focus(state, Some(surf), Serial::from(0u32));
                }
                state.wlr_update_all_states();
            }
            zwlr_foreign_toplevel_handle_v1::Request::Close => {
                if let Some(w) = state.grid.windows.get(&id) {
                    w.surface.send_close();
                }
            }
            zwlr_foreign_toplevel_handle_v1::Request::Destroy => {
                if let Some(handles) = state.wlr_toplevel_state.handles.get_mut(&id) {
                    handles.retain(|h| h != resource);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Request::SetFullscreen { output: _ } => {
                let info = state.grid.windows.get(&id).map(|w| (w.workspace, w.current_state));
                if let Some((_coords, ws)) = info {
                    if !matches!(ws, WindowState::Fullscreen | WindowState::TotalFullscreen) {
                        state.grid.set_focus(id);
                        state.grid.toggle_fullscreen();
                        state.wlr_update_all_states();
                    }
                }
            }
            zwlr_foreign_toplevel_handle_v1::Request::UnsetFullscreen => {
                let info = state.grid.windows.get(&id).map(|w| (w.workspace, w.current_state));
                if let Some((_coords, ws)) = info {
                    if matches!(ws, WindowState::Fullscreen | WindowState::TotalFullscreen) {
                        state.grid.set_focus(id);
                        state.grid.toggle_fullscreen();
                        state.wlr_update_all_states();
                    }
                }
            }
            // No-ops for a tiling compositor
            zwlr_foreign_toplevel_handle_v1::Request::SetMaximized
            | zwlr_foreign_toplevel_handle_v1::Request::UnsetMaximized
            | zwlr_foreign_toplevel_handle_v1::Request::SetMinimized
            | zwlr_foreign_toplevel_handle_v1::Request::UnsetMinimized
            | zwlr_foreign_toplevel_handle_v1::Request::SetRectangle { .. } => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client_id: wayland_server::backend::ClientId,
        resource: &ZwlrForeignToplevelHandleV1,
        window_id: &WindowId,
    ) {
        if let Some(handles) = state.wlr_toplevel_state.handles.get_mut(window_id) {
            handles.retain(|h| h != resource);
        }
    }
}

