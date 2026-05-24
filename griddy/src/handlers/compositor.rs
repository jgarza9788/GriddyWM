use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_output, delegate_shm,
    reexports::wayland_server::{
        Client, Resource,
        protocol::{wl_buffer::WlBuffer, wl_surface::WlSurface},
    },
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState, with_states},
        output::OutputHandler,
        shell::xdg::{SurfaceCachedState, XdgToplevelSurfaceData},
        shm::{ShmHandler, ShmState},
    },
    xwayland::XWaylandClientData,
};

use crate::state::{ClientData, GlobalState};

impl CompositorHandler for GlobalState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(state) = client.get_data::<XWaylandClientData>() {
            return &state.compositor_state;
        }
        &client.get_data::<ClientData>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);

        // Phase 2f: sync app_id / title from the xdg_toplevel role data.
        let window_id = self.grid.window_by_surface(&surface.id());
        if let Some(wid) = window_id {
            let (app_id, title) = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .map(|data| {
                        let guard = data.lock().unwrap();
                        (
                            guard.app_id.clone().unwrap_or_default(),
                            guard.title.clone().unwrap_or_default(),
                        )
                    })
                    .unwrap_or_default()
            });

            // Capture previous values before updating.
            let (prev_app_id, prev_title) = self
                .grid
                .windows
                .get(&wid)
                .map(|w| (w.app_id.clone(), w.title.clone()))
                .unwrap_or_default();

            self.grid
                .update_window_metadata(wid, app_id.clone(), title.clone());

            // Emit change events when title or app_id actually change.
            if !title.is_empty() && title != prev_title {
                self.pending_events
                    .push(crate::ipc::events::Event::WindowTitle {
                        id: wid,
                        title: title.clone(),
                    });
            }
            if !app_id.is_empty() && app_id != prev_app_id {
                self.pending_events
                    .push(crate::ipc::events::Event::WindowAppId {
                        id: wid,
                        app_id: app_id.clone(),
                    });
            }

            // Keep ext-foreign-toplevel-list-v1 handle in sync.
            if let Some(handle) = self.toplevel_handles.get(&wid) {
                if !app_id.is_empty() {
                    handle.send_app_id(&app_id);
                }
                if !title.is_empty() {
                    handle.send_title(&title);
                }
                handle.send_done();
            }
            // Keep wlr-foreign-toplevel-management in sync.
            if !title.is_empty() {
                self.wlr_update_title(wid, &title);
            }
            if !app_id.is_empty() {
                self.wlr_update_app_id(wid, &app_id);
            }

            // §6.13 min_size_overflow: if this tiled window's slot is smaller
            // than its declared minimum, apply the configured overflow policy.
            if matches!(
                self.config.windows.min_size_overflow,
                crate::config::types::MinSizeOverflow::Float
            ) {
                let (min_w, min_h) = with_states(surface, |states| {
                    let ms = states
                        .cached_state
                        .get::<SurfaceCachedState>()
                        .current()
                        .min_size;
                    (ms.w, ms.h)
                });
                if min_w > 0 || min_h > 0 {
                    // Only check tiled windows; floating/fullscreen are exempt.
                    let is_tiled = self
                        .grid
                        .windows
                        .get(&wid)
                        .map(|w| w.current_state == crate::grid::window::WindowState::Tiled)
                        .unwrap_or(false);
                    if is_tiled {
                        let slot_rect = self.grid.compute_rect(wid);
                        if let Some(rect) = slot_rect {
                            let too_narrow = min_w > 0 && rect.w < min_w;
                            let too_short = min_h > 0 && rect.h < min_h;
                            if too_narrow || too_short {
                                let slot_name = self
                                    .grid
                                    .windows
                                    .get(&wid)
                                    .and_then(|w| w.current_slot)
                                    .map(|s| format!("{s:?}"))
                                    .unwrap_or_default();
                                if self.grid.force_min_size_float(wid, min_w, min_h) {
                                    self.pending_events.push(
                                        crate::ipc::events::Event::WindowSizeConstraintFloat {
                                            id: wid,
                                            slot: slot_name,
                                            min_w,
                                            min_h,
                                            slot_w: rect.w,
                                            slot_h: rect.h,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

delegate_compositor!(GlobalState);

impl BufferHandler for GlobalState {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl ShmHandler for GlobalState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

delegate_shm!(GlobalState);

impl OutputHandler for GlobalState {}
delegate_output!(GlobalState);
