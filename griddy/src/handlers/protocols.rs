//! Handler impls for ancillary Wayland protocols:
//!   • cursor-shape-v1
//!   • presentation-time
//!   • idle-inhibit-unstable-v1
//!   • ext-idle-notify-v1
//!   • xdg-activation-v1
//!   • relative-pointer-v1
//!   • pointer-constraints-v1
//!   • viewporter
//!   • single-pixel-buffer-v1
//!   • fractional-scale-v1
//!   • content-type-v1
//!   • wlr-data-control-unstable-v1
//!   • pointer-gestures-unstable-v1  (Session 22)
//!   • keyboard-shortcuts-inhibit-unstable-v1  (Session 22)
//!   • text-input-v3 + input-method-v2  (§22.18)
//!   • security-context-v1  (§16)

use smithay::{
    delegate_content_type, delegate_cursor_shape, delegate_data_control, delegate_fractional_scale,
    delegate_idle_inhibit, delegate_idle_notify, delegate_keyboard_shortcuts_inhibit,
    delegate_pointer_constraints, delegate_pointer_gestures, delegate_presentation,
    delegate_relative_pointer, delegate_single_pixel_buffer, delegate_viewporter,
    delegate_xdg_activation,
    input::pointer::PointerHandle,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Serial},
    wayland::{
        fractional_scale::FractionalScaleHandler,
        idle_inhibit::IdleInhibitHandler,
        idle_notify::{IdleNotifierHandler, IdleNotifierState},
        keyboard_shortcuts_inhibit::{
            KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState,
        },
        pointer_constraints::{PointerConstraintsHandler, with_pointer_constraint},
        selection::wlr_data_control::DataControlHandler,
        tablet_manager::TabletSeatHandler,
        xdg_activation::{
            XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
        },
    },
};
use wayland_server::Resource;

use crate::state::GlobalState;

// ─── cursor-shape-v1 ─────────────────────────────────────────────────────────
// CursorShapeManagerState routes cursor-shape requests through SeatHandler::cursor_image.
// TabletSeatHandler is required by the macro; we use the default (no-op) implementation.
impl TabletSeatHandler for GlobalState {}
delegate_cursor_shape!(GlobalState);

// ─── presentation-time ───────────────────────────────────────────────────────
delegate_presentation!(GlobalState);

// ─── idle-inhibit-unstable-v1 ────────────────────────────────────────────────

impl IdleInhibitHandler for GlobalState {
    fn inhibit(&mut self, _surface: WlSurface) {
        self.idle_inhibit_count = self.idle_inhibit_count.saturating_add(1);
        let inhibited = self.idle_inhibit_count > 0;
        if let Some(n) = &mut self.idle_notifier_state {
            n.set_is_inhibited(inhibited);
        }
        tracing::debug!(count = self.idle_inhibit_count, "Idle inhibitor added");
    }

    fn uninhibit(&mut self, _surface: WlSurface) {
        self.idle_inhibit_count = self.idle_inhibit_count.saturating_sub(1);
        let inhibited = self.idle_inhibit_count > 0;
        if let Some(n) = &mut self.idle_notifier_state {
            n.set_is_inhibited(inhibited);
        }
        tracing::debug!(count = self.idle_inhibit_count, "Idle inhibitor removed");
    }
}

delegate_idle_inhibit!(GlobalState);

// ─── ext-idle-notify-v1 ──────────────────────────────────────────────────────

impl IdleNotifierHandler for GlobalState {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        self.idle_notifier_state
            .as_mut()
            .expect("IdleNotifierState not initialized")
    }
}

delegate_idle_notify!(GlobalState);

// ─── xdg-activation-v1 ───────────────────────────────────────────────────────

impl XdgActivationHandler for GlobalState {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        _token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        // Consume the token immediately to prevent reuse.
        self.xdg_activation_state.remove_token(&token);

        // Find the window that owns this surface and focus it.
        let Some(win_id) = self.grid.window_by_surface(&surface.id()) else {
            return;
        };
        let ws_coords = self.grid.windows.get(&win_id).map(|w| w.workspace);

        // Switch to the window's workspace if it's not the current one.
        if let Some(ws) = ws_coords && ws != self.grid.focused {
            self.grid.switch_workspace(ws);
        }

        self.grid.set_focus(win_id);

        // Update seat keyboard focus.
        let surf_for_seat = self.grid.focused_surface().cloned();
        if let Some(surf) = surf_for_seat {
            let kb = self.seat.get_keyboard().unwrap();
            kb.set_focus(self, Some(surf), Serial::from(0u32));
        }
    }
}

delegate_xdg_activation!(GlobalState);

// ─── relative-pointer-v1 ─────────────────────────────────────────────────────
// PointerHandle::relative_motion() dispatches events to clients automatically.
delegate_relative_pointer!(GlobalState);

// ─── pointer-constraints-v1 ──────────────────────────────────────────────────

impl PointerConstraintsHandler for GlobalState {
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        // Activate the constraint immediately if this surface has keyboard focus.
        let is_focused = self
            .grid
            .focused_surface()
            .map(|s| s == surface)
            .unwrap_or(false);
        if is_focused {
            with_pointer_constraint(surface, pointer, |constraint| {
                if let Some(c) = constraint {
                    c.activate();
                }
            });
        }
    }

    fn cursor_position_hint(
        &mut self,
        _surface: &WlSurface,
        _pointer: &PointerHandle<Self>,
        _location: Point<f64, Logical>,
    ) {
        // Cursor position hints from locked-pointer clients are informational;
        // we don't reposition the cursor based on them in the winit dev backend.
    }
}

delegate_pointer_constraints!(GlobalState);

// ─── viewporter ──────────────────────────────────────────────────────────────
delegate_viewporter!(GlobalState);

// ─── single-pixel-buffer-v1 ──────────────────────────────────────────────────
delegate_single_pixel_buffer!(GlobalState);

// ─── fractional-scale-v1 ─────────────────────────────────────────────────────
impl FractionalScaleHandler for GlobalState {}
delegate_fractional_scale!(GlobalState);

// ─── content-type-v1 ─────────────────────────────────────────────────────────
delegate_content_type!(GlobalState);

// ─── wlr-data-control-unstable-v1 ────────────────────────────────────────────

impl DataControlHandler for GlobalState {
    fn data_control_state(
        &self,
    ) -> &smithay::wayland::selection::wlr_data_control::DataControlState {
        &self.data_control_state
    }
}

delegate_data_control!(GlobalState);

// ─── pointer-gestures-unstable-v1 ────────────────────────────────────────────
// Clients (e.g. GTK apps, touchpad gesture handlers) use this to receive
// multi-finger swipe, pinch, and hold gesture events.  We register the global
// so the protocol is advertised; actual gesture events would come from libinput
// in the DRM backend (not forwarded in the winit dev backend).
delegate_pointer_gestures!(GlobalState);

// ─── keyboard-shortcuts-inhibit-unstable-v1 ──────────────────────────────────
// Games and VM clients (QEMU/virt-manager) request that the compositor passes
// all keyboard events through without consuming compositor keybinds.

impl KeyboardShortcutsInhibitHandler for GlobalState {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.keyboard_shortcuts_inhibit_state
    }

    fn new_inhibitor(
        &mut self,
        inhibitor: smithay::wayland::keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitor,
    ) {
        // Activate immediately so the client starts receiving raw key events.
        inhibitor.activate();
        tracing::debug!("Keyboard shortcuts inhibitor activated");
    }

    fn inhibitor_destroyed(
        &mut self,
        _inhibitor: smithay::wayland::keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitor,
    ) {
        tracing::debug!("Keyboard shortcuts inhibitor destroyed");
    }
}

delegate_keyboard_shortcuts_inhibit!(GlobalState);

// ─── virtual-keyboard-unstable-v1 (§22.9) ────────────────────────────────────
// Virtual keyboard clients (ydotool, wtype, on-screen keyboard apps) inject
// key events through this protocol.  We register the global permissively; actual
// key-event injection goes through smithay's seat keyboard handling.
smithay::delegate_virtual_keyboard_manager!(GlobalState);

// ─── tablet-v2 (§22.9) ───────────────────────────────────────────────────────
// Graphics tablets (Wacom, Huion) use zwp_tablet_manager_v2 to enumerate tablet
// tools and receive proximity/motion/button events.  TabletSeatHandler is already
// a no-op (required by delegate_cursor_shape!); we just register the additional
// global here.
smithay::delegate_tablet_manager!(GlobalState);

// ─── text-input-v3 (§22.18 IME) ──────────────────────────────────────────────
// Clients (fcitx5, ibus, virtual-keyboard) use zwp_text_input_v3 to communicate
// text input state with the compositor.  The protocol state is managed by
// smithay; we just register the global.
smithay::delegate_text_input_manager!(GlobalState);

// ─── input-method-v2 (§22.18 IME) ────────────────────────────────────────────
// Input method engines (fcitx5, ibus) use zwp_input_method_manager_v2 to
// intercept keyboard events and inject pre-edit / commit text.

impl smithay::wayland::input_method::InputMethodHandler for GlobalState {
    fn new_popup(&mut self, _surface: smithay::wayland::input_method::PopupSurface) {}
    fn dismiss_popup(&mut self, _surface: smithay::wayland::input_method::PopupSurface) {}
    fn popup_repositioned(&mut self, _surface: smithay::wayland::input_method::PopupSurface) {}
    fn parent_geometry(
        &self,
        _parent: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) -> smithay::utils::Rectangle<i32, smithay::utils::Logical> {
        smithay::utils::Rectangle::default()
    }
}

smithay::delegate_input_method_manager!(GlobalState);

// ─── security-context-v1 (§16) ───────────────────────────────────────────────
// Sandboxed clients (Flatpak, snap) can be denied access to privileged globals.
// We register the global with a permissive filter for now; deny logic can be
// layered in later by inspecting `SecurityContext::sandbox_engine`.

impl smithay::wayland::security_context::SecurityContextHandler for GlobalState {
    fn context_created(
        &mut self,
        _source: smithay::wayland::security_context::SecurityContextListenerSource,
        context: smithay::wayland::security_context::SecurityContext,
    ) {
        tracing::debug!(
            engine = ?context.sandbox_engine,
            "security-context-v1: new sandboxed client context"
        );
        // Drop source — we don't insert it into the event loop yet.
        // Full sandboxing would register `source` with calloop and restrict
        // protocol access for clients connecting through it.
        let _ = context;
    }
}

smithay::delegate_security_context!(GlobalState);
