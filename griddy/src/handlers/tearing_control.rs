//! `wp-tearing-control-v1` stub (§3 required protocols).
//!
//! Clients (games, drawing apps) use this to opt-in to tearing / async flips.
//! We register the global and track the hint per-surface for future DRM use.
//! Actual tearing requires the DRM backend; in the winit backend this is a no-op.

use wayland_protocols::wp::tearing_control::v1::server::{
    wp_tearing_control_manager_v1::{self, WpTearingControlManagerV1},
    wp_tearing_control_v1::{self, WpTearingControlV1},
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New};

use crate::state::GlobalState;

/// Unit state — the global carries no per-compositor data.
pub struct TearingControlManagerState;

impl TearingControlManagerState {
    pub fn new(dh: &DisplayHandle) -> Self {
        dh.create_global::<GlobalState, WpTearingControlManagerV1, ()>(1, ());
        Self
    }
}

impl GlobalDispatch<WpTearingControlManagerV1, ()> for GlobalState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<WpTearingControlManagerV1>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WpTearingControlManagerV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &WpTearingControlManagerV1,
        request: wp_tearing_control_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_tearing_control_manager_v1::Request::GetTearingControl { id, .. } => {
                data_init.init(id, ());
            }
            wp_tearing_control_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<WpTearingControlV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &WpTearingControlV1,
        request: wp_tearing_control_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_tearing_control_v1::Request::SetPresentationHint { hint } => {
                tracing::debug!(
                    ?hint,
                    "wp_tearing_control_v1: set_presentation_hint (no-op in winit)"
                );
            }
            wp_tearing_control_v1::Request::Destroy => {}
            _ => {}
        }
    }
}
