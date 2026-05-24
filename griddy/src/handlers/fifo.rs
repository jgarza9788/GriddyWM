//! `wp-fifo-v1` stub (§3 required protocols).
//!
//! Frame-pacing hint protocol. Clients call set_barrier / wait_barrier to
//! signal rendering boundaries for latency reduction. In the winit backend
//! all requests are accepted as no-ops; the DRM backend can honour them via
//! KMS commit ordering.

use wayland_protocols::wp::fifo::v1::server::{
    wp_fifo_manager_v1::{self, WpFifoManagerV1},
    wp_fifo_v1::{self, WpFifoV1},
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New};

use crate::state::GlobalState;

pub struct WpFifoManagerState;

impl WpFifoManagerState {
    pub fn new(dh: &DisplayHandle) -> Self {
        dh.create_global::<GlobalState, WpFifoManagerV1, ()>(1, ());
        Self
    }
}

impl GlobalDispatch<WpFifoManagerV1, ()> for GlobalState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<WpFifoManagerV1>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WpFifoManagerV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &WpFifoManagerV1,
        request: wp_fifo_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_fifo_manager_v1::Request::GetFifo { id, .. } => {
                data_init.init(id, ());
            }
            wp_fifo_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<WpFifoV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &WpFifoV1,
        request: wp_fifo_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_fifo_v1::Request::SetBarrier => {
                tracing::trace!("wp_fifo_v1: set_barrier (no-op in winit)");
            }
            wp_fifo_v1::Request::WaitBarrier => {
                tracing::trace!("wp_fifo_v1: wait_barrier (no-op in winit)");
            }
            wp_fifo_v1::Request::Destroy => {}
            _ => {}
        }
    }
}
