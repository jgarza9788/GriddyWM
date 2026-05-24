//! ext-workspace-v1 (Phase 6).
//!
//! Exposes the NxN workspace grid to shell clients (waybar, eww, etc.) so they
//! can display workspace indicators and activate workspaces.
//!
//! Protocol flow on bind:
//!   1. Manager sends `workspace_group` (new_id) → one group per output.
//!   2. Manager sends `workspace` (new_id) for each grid cell (unassigned).
//!   3. Group sends `workspace` (object) to assign each workspace to the group.
//!   4. Each workspace handle receives: id, name, coordinates, state, capabilities.
//!   5. Manager sends `done`.

use std::collections::HashMap;

use wayland_protocols::ext::workspace::v1::server::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use crate::state::GlobalState;

// ─── State ───────────────────────────────────────────────────────────────────

pub struct ExtWorkspaceState {
    /// All live manager resources for broadcasting updates.
    pub managers: Vec<ExtWorkspaceManagerV1>,
    /// Workspace handles keyed by (col, row), across all bound managers.
    pub workspace_handles: HashMap<(u8, u8), Vec<ExtWorkspaceHandleV1>>,
}

impl ExtWorkspaceState {
    pub fn new() -> Self {
        Self { managers: Vec::new(), workspace_handles: HashMap::new() }
    }
}

// ─── Helper methods on GlobalState ───────────────────────────────────────────

impl GlobalState {
    /// Push state updates for all workspaces to all bound managers.
    /// Call after any workspace navigation.
    pub fn ext_workspace_update_all(&mut self) {
        let focused = self.grid.focused;

        // Update state on all handles
        for ((col, row), handles) in &self.ext_workspace_state.workspace_handles {
            let is_active = focused == (*col, *row);
            let state_bits = if is_active {
                ext_workspace_handle_v1::State::Active
            } else {
                ext_workspace_handle_v1::State::empty()
            };
            for h in handles {
                if h.is_alive() {
                    h.state(state_bits);
                }
            }
        }

        // Signal done to all managers
        for m in &self.ext_workspace_state.managers {
            if m.is_alive() {
                m.done();
            }
        }
    }
}

// ─── GlobalDispatch — initial bind ───────────────────────────────────────────

impl GlobalDispatch<ExtWorkspaceManagerV1, ()> for GlobalState {
    fn bind(
        state: &mut Self,
        dh: &DisplayHandle,
        client: &Client,
        resource: New<ExtWorkspaceManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let manager = data_init.init(resource, ());
        let version = manager.version();

        // Create one workspace group for the single output.
        let Ok(group) =
            client.create_resource::<ExtWorkspaceGroupHandleV1, (), GlobalState>(dh, version, ())
        else {
            state.ext_workspace_state.managers.push(manager);
            return;
        };
        manager.workspace_group(&group);

        // Advertise output to the group (skip if we can't get the WlOutput resource).
        let outputs = state.output.client_outputs(client);
        for wl_out in outputs {
            group.output_enter(&wl_out);
        }

        // Group capabilities: we don't support dynamic workspace creation.
        group.capabilities(
            ext_workspace_group_handle_v1::GroupCapabilities::empty(),
        );

        let cols = state.grid.cols;
        let rows = state.grid.rows;
        let focused = state.grid.focused;

        // Create one workspace handle per grid cell.
        for row in 0..rows {
            for col in 0..cols {
                let Ok(ws_handle) = client
                    .create_resource::<ExtWorkspaceHandleV1, (u8, u8), GlobalState>(
                        dh,
                        version,
                        (col, row),
                    )
                else {
                    continue;
                };

                // Manager creates the workspace (new_id)
                manager.workspace(&ws_handle);
                // Group assigns the workspace (object reference)
                group.workspace_enter(&ws_handle);

                // Stable ID: "col,row"
                ws_handle.id(format!("{col},{row}"));
                // Human-readable name: same format
                ws_handle.name(format!("{col},{row}"));
                // Coordinates: [col, row] packed as little-endian u32 each
                let mut coords = Vec::with_capacity(8);
                coords.extend_from_slice(&(col as u32).to_ne_bytes());
                coords.extend_from_slice(&(row as u32).to_ne_bytes());
                ws_handle.coordinates(coords);
                // State: active bit if this is the focused workspace
                let state_bits = if focused == (col, row) {
                    ext_workspace_handle_v1::State::Active
                } else {
                    ext_workspace_handle_v1::State::empty()
                };
                ws_handle.state(state_bits);
                // Capabilities: activate only
                ws_handle.capabilities(
                    ext_workspace_handle_v1::WorkspaceCapabilities::Activate,
                );

                state
                    .ext_workspace_state
                    .workspace_handles
                    .entry((col, row))
                    .or_default()
                    .push(ws_handle);
            }
        }

        manager.done();
        state.ext_workspace_state.managers.push(manager);
    }
}

// ─── Dispatch — manager requests ─────────────────────────────────────────────

impl Dispatch<ExtWorkspaceManagerV1, ()> for GlobalState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ExtWorkspaceManagerV1,
        request: ext_workspace_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            ext_workspace_manager_v1::Request::Stop => {
                state.ext_workspace_state.managers.retain(|m| m != resource);
                resource.finished();
            }
            ext_workspace_manager_v1::Request::Commit => {
                // Client finished sending its requests; broadcast current state.
                state.ext_workspace_update_all();
            }
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client_id: wayland_server::backend::ClientId,
        resource: &ExtWorkspaceManagerV1,
        _data: &(),
    ) {
        state.ext_workspace_state.managers.retain(|m| m != resource);
    }
}

// ─── Dispatch — group handle requests ────────────────────────────────────────

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &ExtWorkspaceGroupHandleV1,
        request: ext_workspace_group_handle_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            // create_workspace is not supported; ignore.
            ext_workspace_group_handle_v1::Request::CreateWorkspace { .. } => {}
            ext_workspace_group_handle_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

// ─── Dispatch — workspace handle requests ────────────────────────────────────

impl Dispatch<ExtWorkspaceHandleV1, (u8, u8)> for GlobalState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ExtWorkspaceHandleV1,
        request: ext_workspace_handle_v1::Request,
        coords: &(u8, u8),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            ext_workspace_handle_v1::Request::Activate => {
                let target = *coords;
                if target != state.grid.focused {
                    state.grid.switch_workspace(target);
                    state.pending_events.push(crate::ipc::events::Event::WorkspaceChanged {
                        col: target.0,
                        row: target.1,
                    });
                    state.ext_workspace_update_all();
                    state.wlr_update_all_states();
                }
            }
            ext_workspace_handle_v1::Request::Deactivate
            | ext_workspace_handle_v1::Request::Remove => {
                // Not supported; ignore.
            }
            ext_workspace_handle_v1::Request::Assign { .. } => {
                // Not supported; ignore.
            }
            ext_workspace_handle_v1::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client_id: wayland_server::backend::ClientId,
        resource: &ExtWorkspaceHandleV1,
        coords: &(u8, u8),
    ) {
        if let Some(handles) = state.ext_workspace_state.workspace_handles.get_mut(coords) {
            handles.retain(|h| h != resource);
        }
    }
}

