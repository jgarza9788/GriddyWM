//! `wlr-screencopy-unstable-v1` stub (§3 required protocols).
//!
//! Advertises the screencopy global so grim, slurp, and portal tools can
//! connect. In the winit backend, actual pixel capture requires EGL readback
//! which is deferred — copy requests respond with `failed` for now.

use wayland_protocols_wlr::screencopy::v1::server::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New};

use crate::state::GlobalState;

pub struct WlrScreencopyState;

impl WlrScreencopyState {
    pub fn new(dh: &DisplayHandle) -> Self {
        dh.create_global::<GlobalState, ZwlrScreencopyManagerV1, ()>(3, ());
        Self
    }
}

impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for GlobalState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for GlobalState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_screencopy_manager_v1::Request::CaptureOutput { frame, .. }
            | zwlr_screencopy_manager_v1::Request::CaptureOutputRegion { frame, .. } => {
                let frame_obj = data_init.init(frame, ());
                // Advertise ARGB8888 buffer matching the current output size.
                let w = state.grid.output_w as u32;
                let h = state.grid.output_h as u32;
                let stride = w * 4;
                frame_obj.buffer(
                    wayland_server::protocol::wl_shm::Format::Argb8888,
                    w,
                    h,
                    stride,
                );
                frame_obj.buffer_done();
            }
            zwlr_screencopy_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        resource: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_screencopy_frame_v1::Request::Copy { .. }
            | zwlr_screencopy_frame_v1::Request::CopyWithDamage { .. } => {
                // Pixel readback from the EGL framebuffer is deferred.
                // Send failed so clients receive a clean error rather than hanging.
                resource.failed();
            }
            zwlr_screencopy_frame_v1::Request::Destroy => {}
            _ => {}
        }
    }
}
