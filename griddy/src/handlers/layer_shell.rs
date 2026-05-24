use smithay::{
    delegate_layer_shell,
    reexports::wayland_server::protocol::wl_output::WlOutput,
    wayland::shell::wlr_layer::{
        Layer, LayerSurface, LayerSurfaceCachedState, WlrLayerShellHandler, WlrLayerShellState,
    },
};
use smithay::wayland::compositor;
use wayland_server::Resource;

use crate::state::GlobalState;

impl WlrLayerShellHandler for GlobalState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: LayerSurface,
        _output: Option<WlOutput>,
        layer: Layer,
        namespace: String,
    ) {
        tracing::debug!(namespace = %namespace, ?layer, "New layer surface");

        // Read the client's requested geometry.
        let cached = compositor::with_states(surface.wl_surface(), |states| {
            *states
                .cached_state
                .get::<LayerSurfaceCachedState>()
                .current()
        });

        let ow = self.grid.output_w;
        let oh = self.grid.output_h;
        let rect = layer_rect(&cached, ow, oh);

        // Configure the surface with the computed size.
        surface.with_pending_state(|state| {
            state.size = Some((rect.w, rect.h).into());
        });
        surface.send_configure();

        // Store the mapped surface.
        self.layer_surfaces.push(crate::state::MappedLayerSurface {
            surface,
            layer,
            namespace,
        });

        // Recompute grid usable area based on all exclusive zones.
        self.recompute_usable_area();
    }

    fn layer_destroyed(&mut self, surface: LayerSurface) {
        let wl_id = surface.wl_surface().id();
        self.layer_surfaces.retain(|s| s.surface.wl_surface().id() != wl_id);
        self.recompute_usable_area();
        tracing::debug!("Layer surface destroyed");
    }
}

delegate_layer_shell!(GlobalState);

// ─── Geometry helpers ─────────────────────────────────────────────────────────

/// Compute a layer surface's rect from its cached state and output dimensions.
pub fn layer_rect(cached: &LayerSurfaceCachedState, output_w: i32, output_h: i32) -> crate::grid::window::Rect {
    use smithay::wayland::shell::wlr_layer::Anchor;
    use crate::grid::window::Rect;

    let anchor = cached.anchor;
    let req_w = cached.size.w;
    let req_h = cached.size.h;
    let margin = cached.margin;

    // Determine width
    let w = if anchor.contains(Anchor::LEFT | Anchor::RIGHT) {
        output_w - margin.left - margin.right
    } else if req_w > 0 {
        req_w
    } else {
        output_w
    };

    // Determine height
    let h = if anchor.contains(Anchor::TOP | Anchor::BOTTOM) {
        output_h - margin.top - margin.bottom
    } else if req_h > 0 {
        req_h
    } else {
        output_h
    };

    // Determine x
    let x = if anchor.contains(Anchor::LEFT | Anchor::RIGHT) {
        margin.left
    } else if anchor.contains(Anchor::RIGHT) {
        output_w - margin.right - w
    } else if anchor.contains(Anchor::LEFT) {
        margin.left
    } else {
        (output_w - w) / 2
    };

    // Determine y
    let y = if anchor.contains(Anchor::TOP | Anchor::BOTTOM) {
        margin.top
    } else if anchor.contains(Anchor::BOTTOM) {
        output_h - margin.bottom - h
    } else if anchor.contains(Anchor::TOP) {
        margin.top
    } else {
        (output_h - h) / 2
    };

    Rect::new(x, y, w.max(1), h.max(1))
}
