//! ext-session-lock-v1 handler.
//!
//! Lock flow (§5.1 of the ext-session-lock-v1 spec):
//!   1. `lock()` is called — set `is_locked = true`, store the `SessionLocker`
//!      in `pending_lock_confirmation` (do NOT call `.lock()` yet).
//!   2. The lock client creates a surface via `new_surface()` — once at least one
//!      output is covered, `confirmation.lock()` is called and `SessionLocked` is
//!      emitted.  This ordering satisfies the protocol requirement that the locked
//!      event is only sent after all outputs are covered.
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
        // Don't send the `locked` protocol event yet — defer until a lock surface
        // actually covers the output (ext-session-lock-v1 §5.1).
        self.pending_lock_confirmation = Some(confirmation);
        tracing::info!("Session lock requested; awaiting lock surface");
    }

    fn unlock(&mut self) {
        self.is_locked = false;
        self.pending_lock_confirmation = None;
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

        // Now that at least one output is covered, confirm the lock.
        if let Some(confirmation) = self.pending_lock_confirmation.take() {
            confirmation.lock();
            self.pending_events.push(Event::SessionLocked);
            tracing::info!("Session locked (output covered)");
        }
    }
}

delegate_session_lock!(GlobalState);
