use smithay::{
    delegate_data_device,
    wayland::selection::{
        SelectionHandler,
        data_device::{
            ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
        },
    },
};

use crate::state::GlobalState;

impl SelectionHandler for GlobalState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for GlobalState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for GlobalState {}
impl ServerDndGrabHandler for GlobalState {}

delegate_data_device!(GlobalState);
