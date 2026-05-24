//! ext-session-lock-v1 handler.
//!
//! When a lock client (swaylock, hyprlock, gtklock) requests a lock:
//!   1. `lock()` is called — set `is_locked = true`, send configure to all
//!      existing lock surfaces, call `confirmation.lock()`.
//!   2. New lock surfaces arrive via `new_surface()`.
//!   3. On unlock (`ext_session_lock_v1.unlock` from the client), `unlock()` is
//!      called — `is_locked = false`, surfaces cleared.
//!
//! The render loop checks `state.is_locked` and renders only lock surfaces when
//! the session is locked (see `backend/winit.rs`).

use smithay::{
    delegate_session_lock,
    reexports::wayland_server::protocol::wl_output::WlOutput,
    wayland::session_lock::{LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker},
};

use crate::ipc::events::Event;
use crate::state::GlobalState;

impl SessionLockHandler for GlobalState {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock_state
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        self.is_locked = true;
        let w = self.grid.output_w as u32;
        let h = self.grid.output_h as u32;
        for surface in &self.lock_surfaces {
            surface.with_pending_state(|s| s.size = Some((w, h).into()));
            surface.send_configure();
        }
        confirmation.lock();
        self.pending_events.push(Event::SessionLocked);
        tracing::info!("Session locked");
    }

    fn unlock(&mut self) {
        self.is_locked = false;
        self.lock_surfaces.clear();
        self.pending_events.push(Event::SessionUnlocked);
        tracing::info!("Session unlocked");
    }

    fn new_surface(&mut self, surface: LockSurface, _output: WlOutput) {
        let w = self.grid.output_w as u32;
        let h = self.grid.output_h as u32;
        surface.with_pending_state(|s| s.size = Some((w, h).into()));
        surface.send_configure();
        self.lock_surfaces.push(surface);
    }
}

delegate_session_lock!(GlobalState);
