//! XWayland integration — X11Wm handler + XWaylandShellHandler.
//!
//! X11 windows are tracked in `state.xwayland_surfaces` as `X11Surface` values.
//! Each mapped X11 window renders at the geometry reported by the X11 WM.
//! Override-redirect windows (menus, tooltips) are treated the same way.

use std::os::unix::io::OwnedFd;

use smithay::{
    delegate_xwayland_shell,
    utils::{Logical, Rectangle},
    wayland::{
        selection::SelectionTarget,
        xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    },
    xwayland::{
        xwm::{ResizeEdge, Reorder, WmWindowProperty, XwmId},
        X11Surface, X11Wm, XwmHandler,
    },
};

use crate::state::GlobalState;

// ─── XwmHandler ──────────────────────────────────────────────────────────────

impl XwmHandler for GlobalState {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.x11_wm.as_mut().expect("X11Wm not initialized")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {
        // New but not yet mapped; nothing to do.
    }

    fn new_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        // Override-redirect windows become visible immediately on map.
        if !self.xwayland_surfaces.contains(&window) {
            self.xwayland_surfaces.push(window);
        }
    }

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::debug!(
            title = %window.title(),
            class = %window.class(),
            "X11 window mapped"
        );
        let _ = window.set_mapped(true);
        if !self.xwayland_surfaces.contains(&window) {
            self.xwayland_surfaces.push(window);
        }
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        if !self.xwayland_surfaces.contains(&window) {
            self.xwayland_surfaces.push(window);
        }
    }

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        self.xwayland_surfaces.retain(|s| s != &window);
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        self.xwayland_surfaces.retain(|s| s != &window);
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        let geo = window.geometry();
        let new_rect = Rectangle::new(
            (
                x.unwrap_or(geo.loc.x),
                y.unwrap_or(geo.loc.y),
            )
                .into(),
            (
                w.map(|v| v as i32).unwrap_or(geo.size.w),
                h.map(|v| v as i32).unwrap_or(geo.size.h),
            )
                .into(),
        );
        let _ = window.configure(new_rect);
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _geometry: Rectangle<i32, Logical>,
        _above: Option<u32>,
    ) {
    }

    fn property_notify(&mut self, _xwm: XwmId, _window: X11Surface, _property: WmWindowProperty) {}

    fn maximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let ow = self.grid.output_w;
        let oh = self.grid.output_h;
        let new_rect = Rectangle::new((0, 0).into(), (ow, oh).into());
        let _ = window.configure(new_rect);
    }

    fn fullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let ow = self.grid.output_w;
        let oh = self.grid.output_h;
        let _ = window.configure(Rectangle::new((0, 0).into(), (ow, oh).into()));
    }

    fn unfullscreen_request(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn minimize_request(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _button: u32,
        _resize_edge: ResizeEdge,
    ) {
    }

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {}

    fn send_selection(
        &mut self,
        _xwm: XwmId,
        _selection: SelectionTarget,
        _mime_type: String,
        _fd: OwnedFd,
    ) {
    }

    fn disconnected(&mut self, _xwm: XwmId) {
        self.x11_wm = None;
        self.xwayland_surfaces.clear();
        tracing::info!("XWayland disconnected");
    }
}

// ─── XWaylandShellHandler ────────────────────────────────────────────────────

impl XWaylandShellHandler for GlobalState {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }

    fn surface_associated(
        &mut self,
        _xwm: XwmId,
        _wl_surface: smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        _surface: X11Surface,
    ) {
        // Surface association is handled automatically by X11Wm.
    }
}

delegate_xwayland_shell!(GlobalState);
