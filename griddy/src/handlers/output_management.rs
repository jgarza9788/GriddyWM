//! `wlr-output-management-unstable-v1` stub (§3 required protocols).
//!
//! Allows kanshi, wlr-randr, and nwg-displays to query the current output
//! configuration. We advertise exactly one virtual output head. Apply/test
//! requests are always cancelled — real mode-setting requires the DRM backend.

use wayland_protocols_wlr::output_management::v1::server::{
    zwlr_output_configuration_head_v1::{self, ZwlrOutputConfigurationHeadV1},
    zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
    zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
    zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
    zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
};
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};

use crate::state::GlobalState;

/// Tracks manager objects for broadcasting changes.
#[derive(Default)]
pub struct WlrOutputManagerState {
    pub managers: Vec<ZwlrOutputManagerV1>,
    pub serial: u32,
}

impl WlrOutputManagerState {
    pub fn new(dh: &DisplayHandle) -> Self {
        dh.create_global::<GlobalState, ZwlrOutputManagerV1, ()>(4, ());
        Self { managers: Vec::new(), serial: 1 }
    }
}

impl GlobalDispatch<ZwlrOutputManagerV1, ()> for GlobalState {
    fn bind(
        state: &mut Self,
        dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrOutputManagerV1>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let manager = data_init.init(resource, ());
        let serial = state.wlr_output_manager_state
            .as_ref()
            .map(|s| s.serial)
            .unwrap_or(1);

        // Advertise our single virtual output head.
        let ow = state.grid.output_w;
        let oh = state.grid.output_h;
        let client = manager.client().unwrap();

        // Create mode object.
        if let Ok(mode) = client.create_resource::<ZwlrOutputModeV1, (), GlobalState>(
            dh, manager.version().min(4), ()
        ) {
            // Create head object.
            if let Ok(head) = client.create_resource::<ZwlrOutputHeadV1, (), GlobalState>(
                dh, manager.version().min(4), ()
            ) {
                manager.head(&head);
                head.name("HEADLESS-1".into());
                head.description("Virtual Wayland Output".into());
                head.physical_size(0, 0); // unknown for virtual output
                head.mode(&mode);
                mode.size(ow, oh);
                mode.refresh(60_000); // 60 Hz in mHz
                mode.preferred();
                head.current_mode(&mode);
                head.enabled(1);
                head.position(0, 0);
                // Transform::Normal = 0 in wl_output enum
                head.transform(wayland_server::protocol::wl_output::Transform::Normal);
                // scale is wl_fixed (24.8 fixed-point i32); 1.0 = 256
                head.scale(1.0f64);
            }
        }

        manager.done(serial);

        if let Some(ref mut s) = state.wlr_output_manager_state {
            s.managers.push(manager);
        }
    }
}

impl Dispatch<ZwlrOutputManagerV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &ZwlrOutputManagerV1,
        request: zwlr_output_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_manager_v1::Request::CreateConfiguration { id, .. } => {
                let cfg = data_init.init(id, ());
                cfg.cancelled();
            }
            zwlr_output_manager_v1::Request::Stop => {}
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputHeadV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &ZwlrOutputHeadV1,
        _request: zwlr_output_head_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {}
}

impl Dispatch<ZwlrOutputModeV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &ZwlrOutputModeV1,
        _request: zwlr_output_mode_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {}
}

impl Dispatch<ZwlrOutputConfigurationV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        resource: &ZwlrOutputConfigurationV1,
        request: zwlr_output_configuration_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_configuration_v1::Request::EnableHead { id, .. } => {
                data_init.init(id, ());
                resource.cancelled();
            }
            zwlr_output_configuration_v1::Request::DisableHead { .. } => {
                resource.cancelled();
            }
            zwlr_output_configuration_v1::Request::Apply
            | zwlr_output_configuration_v1::Request::Test => {
                resource.cancelled();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputConfigurationHeadV1, ()> for GlobalState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &ZwlrOutputConfigurationHeadV1,
        _request: zwlr_output_configuration_head_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {}
}
