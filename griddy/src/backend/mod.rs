pub mod winit;

use anyhow::Result;
use smithay::reexports::calloop::EventLoop;
use wayland_server::Display;

use crate::state::GlobalState;

pub enum BackendType {
    Winit,
    // Drm will be added in Phase 4
}

pub fn run_backend(
    backend_type: BackendType,
    event_loop: EventLoop<'static, GlobalState>,
    display: Display<GlobalState>,
    state: GlobalState,
    no_xwayland: bool,
) -> Result<()> {
    match backend_type {
        BackendType::Winit => winit::run(event_loop, display, state, no_xwayland),
    }
}
