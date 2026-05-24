//! Winit backend — development compositor inside an existing desktop session.
//!
//! Main loop:
//!   1. Poll winit events
//!   2. Dispatch calloop (non-blocking)
//!   3. Render frame (borders + windows; slide animation if active)
//!   4. Accept new Wayland clients
//!   5. Dispatch + flush Wayland display
//!   6. Submit frame
//!   7. Periodic config hot-reload check (§2b)
//!   8. IPC: accept subscribers, handle commands, flush events

use std::{sync::Arc, time::Duration, time::Instant};

use anyhow::{Context, Result};
use smithay::{
    backend::{
        input::{
            AbsolutePositionEvent, Axis, ButtonState, Event, InputEvent, KeyState,
            KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, TouchEvent,
        },
        renderer::{
            Color32F, Frame, Renderer,
            element::{
                Kind,
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
            gles::GlesRenderer,
            utils::draw_render_elements,
        },
        winit::{self, WinitEvent},
    },
    input::{
        keyboard::FilterResult,
        pointer::{AxisFrame, ButtonEvent, MotionEvent, RelativeMotionEvent},
        touch::{DownEvent as TouchDownEvt, MotionEvent as TouchMotionEvt, UpEvent as TouchUpEvt},
    },
    reexports::{calloop::EventLoop, wayland_server::protocol::wl_surface::WlSurface},
    utils::{Logical, Physical, Point, Rectangle, SERIAL_COUNTER, Transform},
    wayland::{
        keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitorSeat,
        shell::wlr_layer::{Layer, LayerSurfaceCachedState},
    },
};
use wayland_server::{Display, ListeningSocket};

use crate::grid::layout;
use crate::grid::window::{Slot, WindowState};
use crate::handlers::layer_shell::layer_rect;
use crate::ipc::commands;
use crate::keybind::Action;
use crate::keybind::dispatcher;
use crate::state::{ClientData, DragKind, DragState, GlobalState};

// Overview gap and margin constants (§7.2, §overview_config).
const OV_GAP_PX: i32 = 16;
const OV_MARGIN_PX: i32 = 32;

// Minimap constants (§7.4).
const MM_CELL_PX: i32 = 14;
const MM_GAP_PX: i32 = 3;
const MM_MARGIN_PX: i32 = 16;

// XKB keysym constants used for overview keyboard navigation.
const XKB_KEY_LEFT: u32 = 0xff51;
const XKB_KEY_RIGHT: u32 = 0xff53;
const XKB_KEY_UP: u32 = 0xff52;
const XKB_KEY_DOWN: u32 = 0xff54;
const XKB_KEY_RETURN: u32 = 0xff0d;
const XKB_KEY_KP_ENTER: u32 = 0xff8d;
const XKB_KEY_ESCAPE: u32 = 0xff1b;
const XKB_KEY_TAB: u32 = 0xff09;
const XKB_KEY_SPACE: u32 = 0x0020;

/// How often to check the config file for changes.
const CONFIG_CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// Mouse button codes.
const BTN_LEFT: u32 = 272;
const BTN_RIGHT: u32 = 273;

pub fn run(
    mut event_loop: EventLoop<'static, GlobalState>,
    mut display: Display<GlobalState>,
    mut state: GlobalState,
    no_xwayland: bool,
) -> Result<()> {
    // ── Initialize winit backend ─────────────────────────────────────────────
    // Must happen BEFORE we overwrite WAYLAND_DISPLAY — winit reads that env
    // var to connect to the host compositor (e.g. niri). If we set it to our
    // own socket first, winit deadlocks trying to connect to a server that
    // hasn't started its event loop yet.
    let (mut backend, mut winit_evloop) = winit::init::<GlesRenderer>()
        .map_err(|e| anyhow::anyhow!("Failed to initialize winit backend: {e:?}"))?;

    // ── Bind Wayland socket ───────────────────────────────────────────────────
    let socket_name = "wayland-griddy";
    let listener = ListeningSocket::bind(socket_name)
        .with_context(|| format!("Failed to bind {socket_name}"))?;
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", socket_name);
    }
    tracing::info!("Wayland socket: {socket_name}");

    let output_size = backend.window_size();
    tracing::info!("Winit window: {}x{}", output_size.w, output_size.h);
    state.grid.set_output_size(output_size.w, output_size.h);

    // Update advertised output mode to actual winit window size.
    let mode = smithay::output::Mode {
        size: (output_size.w, output_size.h).into(),
        refresh: 60_000,
    };
    state
        .output
        .change_current_state(Some(mode), None, None, Some((0, 0).into()));

    // ── XWayland ─────────────────────────────────────────────────────────────
    if !no_xwayland {
        let lh = event_loop.handle();
        let lh_for_wm = lh.clone();
        match smithay::xwayland::XWayland::spawn(
            &display.handle(),
            None,
            std::iter::empty::<(std::ffi::OsString, std::ffi::OsString)>(),
            true,
            std::process::Stdio::null(),
            std::process::Stdio::null(),
            |_| {},
        ) {
            Ok((xwayland, xw_client)) => {
                state.xwayland_client = Some(xw_client);
                if let Err(e) =
                    lh.insert_source(xwayland, move |event, _, state: &mut GlobalState| {
                        use smithay::xwayland::{X11Wm, XWaylandEvent};
                        match event {
                            XWaylandEvent::Ready {
                                x11_socket,
                                display_number,
                                ..
                            } => {
                                let client = state
                                    .xwayland_client
                                    .take()
                                    .expect("xwayland_client already taken");
                                match X11Wm::start_wm(lh_for_wm.clone(), x11_socket, client) {
                                    Ok(wm) => {
                                        state.x11_wm = Some(wm);
                                        unsafe {
                                            std::env::set_var(
                                                "DISPLAY",
                                                format!(":{}", display_number),
                                            );
                                        }
                                        tracing::info!(
                                            "XWayland ready on DISPLAY=:{}",
                                            display_number
                                        );
                                    }
                                    Err(e) => tracing::error!("X11Wm::start_wm failed: {e}"),
                                }
                            }
                            XWaylandEvent::Error => {
                                tracing::warn!("XWayland exited/errored");
                                state.x11_wm = None;
                                state.xwayland_surfaces.clear();
                            }
                        }
                    })
                {
                    tracing::warn!("Failed to register XWayland event source: {e}");
                }
            }
            Err(e) => tracing::warn!("Failed to spawn XWayland: {e}"),
        }
    }

    // ── ext-idle-notify-v1 ───────────────────────────────────────────────────
    {
        let lh = event_loop.handle();
        state.idle_notifier_state = Some(smithay::wayland::idle_notify::IdleNotifierState::new(
            &display.handle(),
            lh,
        ));
    }

    // ── SIGUSR1 → force config reload ────────────────────────────────────────
    {
        use smithay::reexports::calloop::signals::{Signal, Signals};
        match Signals::new(&[Signal::SIGUSR1]) {
            Ok(sig_src) => {
                if let Err(e) =
                    event_loop
                        .handle()
                        .insert_source(sig_src, |_, _, state: &mut GlobalState| {
                            tracing::info!("SIGUSR1: reloading config");
                            state.reload_config_if_changed(true);
                        })
                {
                    tracing::warn!("Failed to register SIGUSR1 source: {e}");
                }
            }
            Err(e) => tracing::warn!("Failed to create SIGUSR1 signal source: {e}"),
        }
    }

    // ── Startup commands ─────────────────────────────────────────────────────
    state.run_startup_cmds();

    let start_time = Instant::now();
    let mut clients = Vec::new();
    let mut winit_closed = false;
    let mut last_config_check = Instant::now();

    // ── Main loop ─────────────────────────────────────────────────────────────
    loop {
        if winit_closed || state.should_exit {
            tracing::info!("Exiting main loop");
            break;
        }

        // 1. Poll winit events
        winit_evloop.dispatch_new_events(|event| {
            if matches!(event, WinitEvent::CloseRequested) {
                winit_closed = true;
            }
            handle_winit_event(event, &mut state);
        });

        // 2. Dispatch calloop (non-blocking)
        event_loop
            .dispatch(Some(Duration::ZERO), &mut state)
            .context("Calloop dispatch error")?;

        // Plugin: frame_begin (§13)
        for plugin in &mut state.plugins {
            plugin.call_frame_begin();
        }

        // 3. Render
        let output_size = backend.window_size();
        if output_size.w != state.grid.output_w || output_size.h != state.grid.output_h {
            state.grid.set_output_size(output_size.w, output_size.h);
            // Update the advertised output mode so bars/clients see the new geometry.
            let mode = smithay::output::Mode {
                size: (output_size.w, output_size.h).into(),
                refresh: 60_000,
            };
            state
                .output
                .change_current_state(Some(mode), None, None, None);
        }

        // Advance / expire slide animation
        if let Some(ref anim) = state.slide_anim
            && anim.is_done()
        {
            state.slide_anim = None;
        }

        // Expire overview animation; flip is_overview when close finishes
        if let Some(ref anim) = state.overview_anim
            && anim.is_done()
        {
            let was_opening = anim.opening;
            state.overview_anim = None;
            if !was_opening {
                state.is_overview = false;
            }
        }

        let screen_rect: Rectangle<i32, Physical> =
            Rectangle::new((0, 0).into(), (output_size.w, output_size.h).into());
        let time_ms = start_time.elapsed().as_millis() as u32;

        // ── Collect render data (owned, no borrows held after this block) ──────
        struct RenderItem {
            surface: WlSurface,
            rect: crate::grid::window::Rect,
            window_id: crate::grid::window::WindowId,
            is_focused: bool,
            is_fullscreen: bool,
            is_urgent: bool,
            /// Number of windows in this window's slot stack (1 = no stack peers).
            stack_depth: usize,
            /// Per-window opacity from rules (1.0 = fully opaque).
            opacity: f32,
            /// Per-window border color override from rules (e.g. "#ff0000").
            border_color: Option<String>,
        }

        // In overview or overview-peek mode we don't render individual window surfaces.
        let in_overview = state.is_overview || state.is_overview_peeking;
        let render_items: Vec<RenderItem> = if !in_overview {
            let new_offset = state
                .slide_anim
                .as_ref()
                .map(|a| a.new_ws_offset(state.grid.output_w, state.grid.output_h))
                .unwrap_or((0, 0));

            let old_info = state.slide_anim.as_ref().map(|a| {
                (
                    a.old_workspace,
                    a.old_ws_offset(state.grid.output_w, state.grid.output_h),
                )
            });

            let focused_id = state.grid.focused_window;
            let mut items = Vec::new();

            if let Some((old_ws, old_off)) = old_info {
                for (w, r) in state.grid.visible_windows_for_ws(old_ws) {
                    let stack_depth = w
                        .current_slot
                        .map(|s| state.grid.slot_stack_ids(w.workspace, s).len())
                        .unwrap_or(1);
                    items.push(RenderItem {
                        surface: w.surface.wl_surface().clone(),
                        rect: crate::grid::window::Rect::new(
                            r.x + old_off.0,
                            r.y + old_off.1,
                            r.w,
                            r.h,
                        ),
                        window_id: w.id,
                        is_focused: focused_id == Some(w.id),
                        is_fullscreen: matches!(
                            w.current_state,
                            WindowState::Fullscreen | WindowState::TotalFullscreen
                        ),
                        is_urgent: w.is_urgent,
                        stack_depth,
                        opacity: w.opacity,
                        border_color: w.border_color.clone(),
                    });
                }
            }
            // Main workspace windows + sticky windows from all workspaces.
            let mut seen_ids = std::collections::HashSet::new();
            for (w, r) in state.grid.visible_windows() {
                seen_ids.insert(w.id);
                let effective_rect = state
                    .move_anims
                    .get(&w.id)
                    .map(|ma| ma.lerp_rect(r))
                    .unwrap_or(r);
                let stack_depth = w
                    .current_slot
                    .map(|s| state.grid.slot_stack_ids(w.workspace, s).len())
                    .unwrap_or(1);
                items.push(RenderItem {
                    surface: w.surface.wl_surface().clone(),
                    rect: crate::grid::window::Rect::new(
                        effective_rect.x + new_offset.0,
                        effective_rect.y + new_offset.1,
                        effective_rect.w,
                        effective_rect.h,
                    ),
                    window_id: w.id,
                    is_focused: focused_id == Some(w.id),
                    is_fullscreen: matches!(
                        w.current_state,
                        WindowState::Fullscreen | WindowState::TotalFullscreen
                    ),
                    is_urgent: w.is_urgent,
                    stack_depth,
                    opacity: w.opacity,
                    border_color: w.border_color.clone(),
                });
            }
            // Sticky windows not already rendered (from other workspaces).
            for w in state.grid.windows.values() {
                if w.sticky
                    && !seen_ids.contains(&w.id)
                    && !w.is_swallowed
                    && let Some(r) = state.grid.compute_rect(w.id)
                {
                    let effective_rect = state
                        .move_anims
                        .get(&w.id)
                        .map(|ma| ma.lerp_rect(r))
                        .unwrap_or(r);
                    items.push(RenderItem {
                        surface: w.surface.wl_surface().clone(),
                        rect: effective_rect,
                        window_id: w.id,
                        is_focused: false,
                        is_fullscreen: false,
                        is_urgent: w.is_urgent,
                        stack_depth: 1,
                        opacity: w.opacity,
                        border_color: w.border_color.clone(),
                    });
                }
            }
            items
        } else {
            vec![]
        };

        // Expire finished open and move animations.
        state.open_anims.retain(|_, a| a.progress().is_some());
        state.move_anims.retain(|_, a| !a.is_done());
        // Clean up dead layer surfaces.
        state.layer_surfaces.retain(|m| m.surface.alive());

        // Frame-callback surfaces: in overview/peek, notify ALL workspaces so clients
        // don't stall waiting for callbacks.
        let frame_surfaces: Vec<WlSurface> = if in_overview {
            let mut surfs = Vec::new();
            for row in 0..state.grid.rows {
                for col in 0..state.grid.cols {
                    for (w, _) in state.grid.visible_windows_for_ws((col, row)) {
                        surfs.push(w.surface.wl_surface().clone());
                    }
                }
            }
            surfs
        } else {
            let mut surfs: Vec<WlSurface> = state
                .grid
                .visible_windows()
                .iter()
                .map(|(w, _)| w.surface.wl_surface().clone())
                .collect();
            if let Some(old_ws) = state.slide_anim.as_ref().map(|a| a.old_workspace) {
                for (w, _) in state.grid.visible_windows_for_ws(old_ws) {
                    surfs.push(w.surface.wl_surface().clone());
                }
            }
            surfs
        };

        let border_focused = state.config.theme.window.focused.border_px;
        let border_unfocused = state.config.theme.window.unfocused.border_px;
        let accent = parse_hex_color(&state.config.theme.colors.accent);
        let accent_dim = parse_hex_color(&state.config.theme.colors.accent_dim);
        let idle = parse_hex_color(&state.config.theme.colors.border_idle);
        let warn = parse_hex_color(&state.config.theme.colors.warn);
        let fg_dim = parse_hex_color(&state.config.theme.colors.fg_dim);
        let bg = parse_hex_color(&state.config.theme.colors.bg);
        let bg_alt = parse_hex_color(&state.config.theme.colors.bg_alt);
        let ok_color = parse_hex_color(&state.config.theme.colors.ok);
        let danger = parse_hex_color(&state.config.theme.colors.danger);

        {
            let (renderer, mut framebuffer) = backend
                .bind()
                .map_err(|e| anyhow::anyhow!("Failed to bind framebuffer: {e:?}"))?;

            // Phase 1: pre-build surface elements (focus mode only; skipped in overview).
            struct WindowRenderData {
                elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
                rect: crate::grid::window::Rect,
                is_focused: bool,
                is_fullscreen: bool,
                is_urgent: bool,
                border_px: i32,
                alpha: f32,
                stack_depth: usize,
                /// Rule-overridden border color (RGBA floats), if any.
                override_border: Option<[f32; 4]>,
            }
            let window_data: Vec<WindowRenderData> = render_items
                .iter()
                .map(|item| {
                    let bpx = if item.is_focused {
                        border_focused
                    } else {
                        border_unfocused
                    };
                    let anim_alpha = state
                        .open_anims
                        .get(&item.window_id)
                        .and_then(|a| a.progress())
                        .unwrap_or(1.0);
                    // Multiply animation alpha by per-window opacity from rules.
                    let alpha = (anim_alpha * item.opacity).clamp(0.0, 1.0);
                    let elements = render_elements_from_surface_tree(
                        renderer,
                        &item.surface,
                        (item.rect.x, item.rect.y),
                        1.0,
                        alpha,
                        Kind::Unspecified,
                    );
                    let override_border = item
                        .border_color
                        .as_deref()
                        .map(parse_hex_color)
                        .map(|c| [c[0], c[1], c[2], alpha]);
                    WindowRenderData {
                        elements,
                        rect: item.rect,
                        is_focused: item.is_focused,
                        is_fullscreen: item.is_fullscreen,
                        is_urgent: item.is_urgent,
                        border_px: bpx,
                        alpha,
                        stack_depth: item.stack_depth,
                        override_border,
                    }
                })
                .collect();

            // Phase 1b: layer surface elements (Top + Overlay rendered in both modes).
            struct LayerRenderData {
                elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
                layer: Layer,
            }
            let layer_data: Vec<LayerRenderData> = {
                let ow = state.grid.output_w;
                let oh = state.grid.output_h;
                state
                    .layer_surfaces
                    .iter()
                    .filter_map(|m| {
                        if !m.surface.alive() {
                            return None;
                        }
                        let cached = smithay::wayland::compositor::with_states(
                            m.surface.wl_surface(),
                            |states| {
                                *states
                                    .cached_state
                                    .get::<LayerSurfaceCachedState>()
                                    .current()
                            },
                        );
                        let r = layer_rect(&cached, ow, oh);
                        let elements = render_elements_from_surface_tree(
                            renderer,
                            m.surface.wl_surface(),
                            (r.x, r.y),
                            1.0,
                            1.0f32,
                            Kind::Unspecified,
                        );
                        Some(LayerRenderData {
                            elements,
                            layer: m.layer,
                        })
                    })
                    .collect()
            };

            // Phase 1c: X11 surface render elements (mapped XWayland windows).
            struct X11RenderData {
                elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
                x: i32,
                y: i32,
            }
            let x11_data: Vec<X11RenderData> = state
                .xwayland_surfaces
                .iter()
                .filter_map(|surf| {
                    let wl = surf.wl_surface()?;
                    let geo = surf.geometry();
                    let elements = render_elements_from_surface_tree(
                        renderer,
                        &wl,
                        (geo.loc.x, geo.loc.y),
                        1.0,
                        1.0f32,
                        Kind::Unspecified,
                    );
                    Some(X11RenderData {
                        elements,
                        x: geo.loc.x,
                        y: geo.loc.y,
                    })
                })
                .collect();

            // Phase 1d: session-lock surface elements (rendered only when locked).
            let lock_data: Vec<Vec<WaylandSurfaceRenderElement<GlesRenderer>>> = if state.is_locked
            {
                state
                    .lock_surfaces
                    .iter()
                    .filter(|s| s.alive())
                    .map(|s| {
                        render_elements_from_surface_tree(
                            renderer,
                            s.wl_surface(),
                            (0, 0),
                            1.0,
                            1.0f32,
                            Kind::Unspecified,
                        )
                    })
                    .collect()
            } else {
                vec![]
            };

            // Phase 1e: stack-peek cascade elements (§6.8).
            // Pre-built while we still hold the renderer (before frame open).
            struct CascadeLayerData {
                elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
                rect: crate::grid::window::Rect,
                is_top: bool,
            }
            let cascade_data: Vec<CascadeLayerData> = if state.is_stack_peeking && !in_overview {
                let mut result = Vec::new();
                let focused_id = state.grid.focused_window;
                if let Some(fid) = focused_id {
                    let info = state
                        .grid
                        .windows
                        .get(&fid)
                        .and_then(|w| Some((w.workspace, w.current_slot?)));
                    if let Some((ws, slot)) = info {
                        let stack_ids = state.grid.slot_stack_ids(ws, slot);
                        if stack_ids.len() > 1 {
                            let smart = state.grid.focused_ws().should_use_smart_gaps();
                            let base = layout::slot_rect(
                                slot,
                                state.grid.usable_w,
                                state.grid.usable_h,
                                state.grid.inner_gap_px,
                                state.grid.outer_gap_px,
                                smart,
                            );
                            let shift_px = state.config.theme.window.stacked.peek_cascade_offset_px;
                            let usable_x = state.grid.usable_x;
                            let usable_y = state.grid.usable_y;
                            for (idx, win_id) in stack_ids.iter().enumerate() {
                                if let Some(win) = state.grid.windows.get(win_id) {
                                    let s = idx as i32 * shift_px;
                                    let rect = crate::grid::window::Rect::new(
                                        base.x + usable_x + s,
                                        base.y + usable_y + s,
                                        base.w,
                                        base.h,
                                    );
                                    let alpha = if idx == 0 {
                                        1.0f32
                                    } else {
                                        state.config.theme.window.stacked.peek_dim_unstacked
                                    };
                                    let wl = win.surface.wl_surface().clone();
                                    let elements = render_elements_from_surface_tree(
                                        renderer,
                                        &wl,
                                        (rect.x, rect.y),
                                        1.0,
                                        alpha,
                                        Kind::Unspecified,
                                    );
                                    result.push(CascadeLayerData {
                                        elements,
                                        rect,
                                        is_top: idx == 0,
                                    });
                                }
                            }
                        }
                    }
                }
                result
            } else {
                vec![]
            };

            // Plugin: pre_render (§13)
            for plugin in &mut state.plugins {
                plugin.call_pre_render();
            }

            // Phase 2: open frame and render.
            let mut frame = renderer
                .render(&mut framebuffer, output_size, Transform::Flipped180)
                .context("Failed to begin frame")?;

            // ── Session-lock rendering — overrides everything else ──────────────
            if state.is_locked && !lock_data.is_empty() {
                frame
                    .clear(Color32F::new(0.0, 0.0, 0.0, 1.0), &[screen_rect])
                    .context("Failed to clear lock background")?;
                for elements in &lock_data {
                    draw_render_elements(&mut frame, 1.0, elements, &[screen_rect]).ok();
                }
                // Skip all normal rendering when locked.
            } else
            // ── Overview / peek mode rendering (§7.2) ─────────────────────────
            if in_overview {
                // Clear to bg color.
                frame
                    .clear(Color32F::new(bg[0], bg[1], bg[2], 1.0), &[screen_rect])
                    .context("Failed to clear overview background")?;

                let ow = state.grid.output_w;
                let oh = state.grid.output_h;
                let cols = state.grid.cols;
                let rows = state.grid.rows;
                let focused_ws = state.grid.focused;
                let ov_focused = state.overview_focused;
                let focused_win = state.grid.focused_window;
                let drag_win = state.overview_drag.as_ref().map(|d| d.window_id);
                let drag_target = state.overview_drag.as_ref().and_then(|d| d.target_ws);

                // Zoom animation: lerp gap/margin from 0 → full so thumbnails
                // appear to spread out from a tiled full-screen layout.
                let ov_t = state
                    .overview_anim
                    .as_ref()
                    .map(|a| a.overview_progress())
                    .unwrap_or(1.0);
                let lerp_i =
                    |a: i32, b: i32| -> i32 { (a as f32 + (b - a) as f32 * ov_t) as i32 };
                let anim_gap = lerp_i(0, OV_GAP_PX);
                let anim_margin = lerp_i(0, OV_MARGIN_PX);

                for ov_row in 0..rows {
                    for ov_col in 0..cols {
                        let thumb = layout::overview_thumbnail_rect(
                            ov_col,
                            ov_row,
                            cols,
                            rows,
                            ow,
                            oh,
                            anim_gap,
                            anim_margin,
                        );
                        let is_home = (ov_col, ov_row) == focused_ws;
                        let is_kb = (ov_col, ov_row) == ov_focused;
                        let is_drag_target = drag_target == Some((ov_col, ov_row));

                        // ── Thumbnail border ──────────────────────────────────
                        let border_px: i32 = if is_drag_target { 4 } else if is_kb { 3 } else { 1 };
                        let bc = if is_drag_target {
                            lighten(accent, 0.25)
                        } else if is_kb {
                            accent
                        } else if is_home {
                            accent_dim
                        } else {
                            idle
                        };
                        let brect: Rectangle<i32, Physical> = Rectangle::new(
                            (thumb.x - border_px, thumb.y - border_px).into(),
                            (thumb.w + 2 * border_px, thumb.h + 2 * border_px).into(),
                        );
                        if let Some(clipped) = brect.intersection(screen_rect) {
                            frame
                                .clear(Color32F::new(bc[0], bc[1], bc[2], 1.0), &[clipped])
                                .ok();
                        }

                        // ── Thumbnail background ──────────────────────────────
                        let thumb_bg = if is_drag_target {
                            lighten(bg_alt, 0.12)
                        } else if is_home {
                            lighten(bg_alt, 0.05)
                        } else {
                            bg_alt
                        };
                        let thumb_phys: Rectangle<i32, Physical> =
                            Rectangle::new((thumb.x, thumb.y).into(), (thumb.w, thumb.h).into());
                        frame
                            .clear(
                                Color32F::new(thumb_bg[0], thumb_bg[1], thumb_bg[2], 1.0),
                                &[thumb_phys],
                            )
                            .ok();

                        // ── Window representations ────────────────────────────
                        let scale_x = thumb.w as f64 / ow as f64;
                        let scale_y = thumb.h as f64 / oh as f64;
                        for (win, rect) in state.grid.visible_windows_for_ws((ov_col, ov_row)) {
                            // Skip the window being dragged (draw it at cursor instead).
                            if drag_win == Some(win.id) {
                                continue;
                            }
                            let wx = thumb.x + (rect.x as f64 * scale_x) as i32;
                            let wy = thumb.y + (rect.y as f64 * scale_y) as i32;
                            let ww = ((rect.w as f64 * scale_x) as i32).max(2);
                            let wh = ((rect.h as f64 * scale_y) as i32).max(2);
                            let win_phys: Rectangle<i32, Physical> =
                                Rectangle::new((wx, wy).into(), (ww, wh).into());
                            let wc = if focused_win == Some(win.id) {
                                accent
                            } else if win.current_state == WindowState::Floating {
                                fg_dim
                            } else {
                                accent_dim
                            };
                            if let Some(clipped) = win_phys.intersection(thumb_phys) {
                                // 1px inner gap so adjacent windows are distinguishable.
                                let inner: Rectangle<i32, Physical> = Rectangle::new(
                                    (clipped.loc.x + 1, clipped.loc.y + 1).into(),
                                    ((clipped.size.w - 2).max(1), (clipped.size.h - 2).max(1))
                                        .into(),
                                );
                                frame
                                    .clear(Color32F::new(wc[0], wc[1], wc[2], 1.0), &[inner])
                                    .ok();
                            }
                        }
                    }
                }

                // ── Overview drag ghost: draw dragged window following cursor ──
                if let Some(ref drag) = state.overview_drag {
                    // Size: proportional to an average thumbnail cell.
                    let ghost_w = (ow / cols as i32 / 4).max(40);
                    let ghost_h = (oh / rows as i32 / 4).max(30);
                    let cx = drag.cursor.0 as i32 - ghost_w / 2;
                    let cy = drag.cursor.1 as i32 - ghost_h / 2;
                    let ghost_phys: Rectangle<i32, Physical> =
                        Rectangle::new((cx, cy).into(), (ghost_w, ghost_h).into());
                    if let Some(clipped) = ghost_phys.intersection(screen_rect) {
                        frame
                            .clear(Color32F::new(accent[0], accent[1], accent[2], 1.0), &[clipped])
                            .ok();
                        // Inner highlight so it looks like a labelled card.
                        let inner: Rectangle<i32, Physical> = Rectangle::new(
                            (clipped.loc.x + 2, clipped.loc.y + 2).into(),
                            ((clipped.size.w - 4).max(1), (clipped.size.h - 4).max(1)).into(),
                        );
                        frame
                            .clear(
                                Color32F::new(
                                    lighten(accent, 0.2)[0],
                                    lighten(accent, 0.2)[1],
                                    lighten(accent, 0.2)[2],
                                    1.0,
                                ),
                                &[inner],
                            )
                            .ok();
                    }
                }

                // ── Layer: Top + Overlay rendered over the overview (bars stay visible) ──
                for ld in layer_data
                    .iter()
                    .filter(|l| matches!(l.layer, Layer::Top | Layer::Overlay))
                {
                    draw_render_elements(&mut frame, 1.0, &ld.elements, &[screen_rect]).ok();
                }

                // ── Minimap HUD (§7.4) — in overview we skip minimap (redundant) ──
            } else {
                // ── Focus mode rendering ────────────────────────────────────────

                frame
                    .clear(Color32F::new(bg[0], bg[1], bg[2], 1.0), &[screen_rect])
                    .context("Failed to clear background")?;

                // Layer: Background
                for ld in layer_data
                    .iter()
                    .filter(|l| matches!(l.layer, Layer::Background))
                {
                    draw_render_elements(&mut frame, 1.0, &ld.elements, &[screen_rect]).ok();
                }
                // Layer: Bottom
                for ld in layer_data
                    .iter()
                    .filter(|l| matches!(l.layer, Layer::Bottom))
                {
                    draw_render_elements(&mut frame, 1.0, &ld.elements, &[screen_rect]).ok();
                }

                for wd in &window_data {
                    // ── Stack depth cue ("deck of cards" §6.8) ────────────────
                    let depth_cfg = &state.config.theme.window.stacked;
                    if depth_cfg.depth_cue && wd.stack_depth > 1 && !wd.is_fullscreen {
                        let offset = depth_cfg.depth_offset_px;
                        let max_layers = depth_cfg.depth_max_layers.min(wd.stack_depth);
                        // Render deepest layers first (they're behind upper layers).
                        for layer in (1..max_layers).rev() {
                            let dx = offset * layer as i32;
                            let dy = offset * layer as i32;
                            // Right strip: vertical strip at the right edge of the slot.
                            let right_strip = Rectangle::<i32, Physical>::new(
                                (wd.rect.x + wd.rect.w - dx, wd.rect.y + dy).into(),
                                (dx, wd.rect.h - dy).into(),
                            );
                            // Bottom strip: horizontal strip at the bottom edge.
                            let bottom_strip = Rectangle::<i32, Physical>::new(
                                (wd.rect.x + dx, wd.rect.y + wd.rect.h - dy).into(),
                                (wd.rect.w - dx, dy).into(),
                            );
                            let cue_color = Color32F::new(
                                idle[0] * 0.85,
                                idle[1] * 0.85,
                                idle[2] * 0.85,
                                wd.alpha * 0.9,
                            );
                            if let Some(c) = right_strip.intersection(screen_rect) {
                                frame.clear(cue_color, &[c]).ok();
                            }
                            if let Some(c) = bottom_strip.intersection(screen_rect) {
                                frame.clear(cue_color, &[c]).ok();
                            }
                        }
                    }

                    if wd.border_px > 0 && !wd.is_fullscreen {
                        let color = if let Some(oc) = wd.override_border {
                            // Per-window border color from rules takes precedence.
                            Color32F::new(oc[0], oc[1], oc[2], oc[3])
                        } else if wd.is_focused {
                            Color32F::new(accent[0], accent[1], accent[2], wd.alpha)
                        } else if wd.is_urgent {
                            Color32F::new(warn[0], warn[1], warn[2], wd.alpha)
                        } else {
                            Color32F::new(idle[0], idle[1], idle[2], wd.alpha)
                        };
                        let bpx = wd.border_px;
                        let brect: Rectangle<i32, Physical> = Rectangle::new(
                            (wd.rect.x - bpx, wd.rect.y - bpx).into(),
                            (wd.rect.w + 2 * bpx, wd.rect.h + 2 * bpx).into(),
                        );
                        if let Some(clipped) = brect.intersection(screen_rect) {
                            frame.clear(color, &[clipped]).ok();
                        }
                    }
                    draw_render_elements(&mut frame, 1.0, &wd.elements, &[screen_rect])
                        .context("Failed to draw window")?;
                }

                // XWayland surfaces — rendered above tiled windows, below Top layer.
                for xd in &x11_data {
                    draw_render_elements(&mut frame, 1.0, &xd.elements, &[screen_rect]).ok();
                }

                // Layer: Top
                for ld in layer_data.iter().filter(|l| matches!(l.layer, Layer::Top)) {
                    draw_render_elements(&mut frame, 1.0, &ld.elements, &[screen_rect]).ok();
                }
                // Layer: Overlay
                for ld in layer_data
                    .iter()
                    .filter(|l| matches!(l.layer, Layer::Overlay))
                {
                    draw_render_elements(&mut frame, 1.0, &ld.elements, &[screen_rect]).ok();
                }

                // Snap preview: translucent slot rect while dragging near an edge.
                if let Some(preview_slot) = state.drag.as_ref().and_then(|d| d.snap_preview) {
                    let smart_gaps = state.grid.focused_ws().should_use_smart_gaps();
                    let preview_rect = layout::slot_rect(
                        preview_slot,
                        state.grid.output_w,
                        state.grid.output_h,
                        state.grid.inner_gap_px,
                        state.grid.outer_gap_px,
                        smart_gaps,
                    );
                    let prect: Rectangle<i32, Physical> = Rectangle::new(
                        (preview_rect.x, preview_rect.y).into(),
                        (preview_rect.w, preview_rect.h).into(),
                    );
                    frame
                        .clear(
                            Color32F::new(accent[0], accent[1], accent[2], 0.25),
                            &[prect],
                        )
                        .ok();
                }

                // ── Minimap HUD (§7.4) ─────────────────────────────────────────
                // Rendered as a small grid in the bottom-right corner.
                {
                    let ow = state.grid.output_w;
                    let oh = state.grid.output_h;
                    let cols = state.grid.cols;
                    let rows = state.grid.rows;
                    let focused_ws = state.grid.focused;
                    // Total minimap size
                    let mm_w = cols as i32 * MM_CELL_PX + (cols as i32 - 1) * MM_GAP_PX;
                    let mm_h = rows as i32 * MM_CELL_PX + (rows as i32 - 1) * MM_GAP_PX;
                    let anchor_x = ow - MM_MARGIN_PX - mm_w;
                    let anchor_y = oh - MM_MARGIN_PX - mm_h;

                    for mm_row in 0..rows {
                        for mm_col in 0..cols {
                            let cell = layout::minimap_cell_rect(
                                mm_col, mm_row, MM_CELL_PX, MM_GAP_PX, anchor_x, anchor_y,
                            );
                            let cell_phys: Rectangle<i32, Physical> =
                                Rectangle::new((cell.x, cell.y).into(), (cell.w, cell.h).into());
                            let is_focused = (mm_col, mm_row) == focused_ws;
                            let has_windows = !state
                                .grid
                                .visible_windows_for_ws((mm_col, mm_row))
                                .is_empty();
                            let cc = if is_focused {
                                accent
                            } else if has_windows {
                                idle
                            } else {
                                bg_alt
                            };
                            if let Some(clipped) = cell_phys.intersection(screen_rect) {
                                frame
                                    .clear(Color32F::new(cc[0], cc[1], cc[2], 0.85), &[clipped])
                                    .ok();
                            }
                        }
                    }
                }

                // ── Stack-peek cascade overlay (§6.8) ─────────────────────────────
                if !cascade_data.is_empty() {
                    // Dim everything else to focus attention on the cascade.
                    frame
                        .clear(Color32F::new(0.0, 0.0, 0.0, 0.5), &[screen_rect])
                        .ok();
                    // Render stack from bottom (most-shifted) to top (least-shifted).
                    for layer in cascade_data.iter().rev() {
                        let bpx = if layer.is_top {
                            border_focused
                        } else {
                            border_unfocused
                        };
                        let bc = if layer.is_top { accent } else { idle };
                        // Background fill so window content is readable.
                        let bg_rect: Rectangle<i32, Physical> = Rectangle::new(
                            (layer.rect.x, layer.rect.y).into(),
                            (layer.rect.w, layer.rect.h).into(),
                        );
                        if let Some(c) = bg_rect.intersection(screen_rect) {
                            frame
                                .clear(Color32F::new(bg[0], bg[1], bg[2], 1.0), &[c])
                                .ok();
                        }
                        // Border.
                        if bpx > 0 {
                            let brect: Rectangle<i32, Physical> = Rectangle::new(
                                (layer.rect.x - bpx, layer.rect.y - bpx).into(),
                                (layer.rect.w + 2 * bpx, layer.rect.h + 2 * bpx).into(),
                            );
                            if let Some(c) = brect.intersection(screen_rect) {
                                frame
                                    .clear(Color32F::new(bc[0], bc[1], bc[2], 1.0), &[c])
                                    .ok();
                            }
                        }
                        // Window surface content.
                        draw_render_elements(&mut frame, 1.0, &layer.elements, &[screen_rect]).ok();
                    }
                }
            }

            // ── OSD indicator (§8.9) ─────────────────────────────────────────────────
            let osd_visible = state
                .osd_hide_at
                .map(|t| t > std::time::Instant::now())
                .unwrap_or(false);
            if osd_visible && state.config.theme.osd.enabled {
                let osd_kind = state.osd_kind.as_deref().unwrap_or("workspace");
                let show_ws = osd_kind == "workspace";
                // Determine bar color for status-style OSD kinds.
                let bar_color: Option<[f32; 4]> = match osd_kind {
                    "submap" => Some(accent),
                    "reload-ok" => Some(ok_color),
                    "reload-err" => Some(danger),
                    "notification-daemon-missing" => Some(warn),
                    "safe-mode" => Some(danger),
                    // Custom osd-show text: render as accent bar.
                    other if other != "workspace" => Some(accent),
                    _ => None,
                };

                let ow = state.grid.output_w;
                let oh = state.grid.output_h;
                let margin = state.config.theme.osd.margin_px;
                let pad = 6i32;

                if show_ws {
                    let cell_px = state.config.theme.osd.cell_px;
                    let gap_px = state.config.theme.osd.cell_gap_px;
                    let cols = state.grid.cols as i32;
                    let rows = state.grid.rows as i32;
                    let osd_w = cols * cell_px + (cols - 1) * gap_px + pad * 2;
                    let osd_h = rows * cell_px + (rows - 1) * gap_px + pad * 2;
                    let osd_x = match state.config.theme.osd.position.as_str() {
                        "top-left" | "bottom-left" => margin,
                        "top-right" | "bottom-right" => ow - margin - osd_w,
                        _ => (ow - osd_w) / 2,
                    };
                    let osd_y = match state.config.theme.osd.position.as_str() {
                        "bottom-left" | "bottom-right" | "bottom-center" => oh - margin - osd_h,
                        "center" => (oh - osd_h) / 2,
                        _ => margin,
                    };
                    // Background panel.
                    let panel = Rectangle::new((osd_x, osd_y).into(), (osd_w, osd_h).into());
                    if let Some(c) = panel.intersection(screen_rect) {
                        frame
                            .clear(Color32F::new(bg_alt[0], bg_alt[1], bg_alt[2], 0.90), &[c])
                            .ok();
                    }
                    let focused_ws = state.grid.focused;
                    for wr in 0..rows {
                        for wc in 0..cols {
                            let cx = osd_x + pad + wc * (cell_px + gap_px);
                            let cy = osd_y + pad + wr * (cell_px + gap_px);
                            let is_focused = (wc as u8, wr as u8) == focused_ws;
                            let has_win = !state
                                .grid
                                .visible_windows_for_ws((wc as u8, wr as u8))
                                .is_empty();
                            let cc = if is_focused {
                                accent
                            } else if has_win {
                                idle
                            } else {
                                bg_alt
                            };
                            let cell_r: Rectangle<i32, Physical> =
                                Rectangle::new((cx, cy).into(), (cell_px, cell_px).into());
                            if let Some(c) = cell_r.intersection(screen_rect) {
                                frame
                                    .clear(
                                        Color32F::new(
                                            cc[0],
                                            cc[1],
                                            cc[2],
                                            if is_focused { 1.0 } else { 0.7 },
                                        ),
                                        &[c],
                                    )
                                    .ok();
                            }
                        }
                    }
                } else if let Some(bc) = bar_color {
                    // Status bar: a fixed-size pill.  Width scales with grid columns,
                    // height is 3× the cell_px so it's visible but compact.
                    let cell_px = state.config.theme.osd.cell_px;
                    let cols = state.grid.cols as i32;
                    let osd_w = (cols * cell_px * 2).max(80);
                    let osd_h = cell_px * 3 + pad * 2;
                    let osd_x = match state.config.theme.osd.position.as_str() {
                        "top-left" | "bottom-left" => margin,
                        "top-right" | "bottom-right" => ow - margin - osd_w,
                        _ => (ow - osd_w) / 2,
                    };
                    let osd_y = match state.config.theme.osd.position.as_str() {
                        "bottom-left" | "bottom-right" | "bottom-center" => oh - margin - osd_h,
                        "center" => (oh - osd_h) / 2,
                        _ => margin,
                    };
                    // Background panel.
                    let panel = Rectangle::new((osd_x, osd_y).into(), (osd_w, osd_h).into());
                    if let Some(c) = panel.intersection(screen_rect) {
                        frame
                            .clear(Color32F::new(bg_alt[0], bg_alt[1], bg_alt[2], 0.90), &[c])
                            .ok();
                    }
                    // Colored inner bar.
                    let inner = Rectangle::new(
                        (osd_x + pad, osd_y + pad).into(),
                        (osd_w - pad * 2, osd_h - pad * 2).into(),
                    );
                    if let Some(c) = inner.intersection(screen_rect) {
                        frame
                            .clear(Color32F::new(bc[0], bc[1], bc[2], 0.90), &[c])
                            .ok();
                    }
                }
            }

            // ── Debug overlay (§22.14) — frame-time bar in top-left corner ────────
            if state.debug_overlay {
                let now = std::time::Instant::now();
                let dt_ms = now.duration_since(state.debug_last_frame).as_secs_f32() * 1000.0;
                state.debug_last_frame = now;
                state.debug_frame_times.push_back(dt_ms);
                if state.debug_frame_times.len() > 60 {
                    state.debug_frame_times.pop_front();
                }
                let avg_ms: f32 = if state.debug_frame_times.is_empty() {
                    16.67
                } else {
                    state.debug_frame_times.iter().sum::<f32>()
                        / state.debug_frame_times.len() as f32
                };
                // Draw a thin bar: green (<20ms), yellow (<33ms), red (>33ms).
                let bar_h = 4i32;
                let bar_w = (state.grid.output_w as f32 * (avg_ms / 50.0).min(1.0)) as i32;
                let bar_color = if avg_ms < 20.0 {
                    [0.0f32, 0.8, 0.3, 1.0]
                } else if avg_ms < 33.0 {
                    [0.9, 0.8, 0.0, 1.0]
                } else {
                    [0.9, 0.2, 0.1, 1.0]
                };
                let dbg_rect: Rectangle<i32, Physical> =
                    Rectangle::new((0, 0).into(), (bar_w.max(1), bar_h).into());
                if let Some(c) = dbg_rect.intersection(screen_rect) {
                    frame
                        .clear(
                            Color32F::new(bar_color[0], bar_color[1], bar_color[2], bar_color[3]),
                            &[c],
                        )
                        .ok();
                }
                // Window count indicator: small square per window in grid (top-left, below bar).
                let win_count = state.grid.windows.len().min(64);
                for i in 0..win_count {
                    let wx = (i as i32 % 16) * 5;
                    let wy = bar_h + (i as i32 / 16) * 5;
                    let wr: Rectangle<i32, Physical> =
                        Rectangle::new((wx, wy).into(), (4, 4).into());
                    if let Some(c) = wr.intersection(screen_rect) {
                        frame.clear(Color32F::new(0.5, 0.8, 1.0, 0.9), &[c]).ok();
                    }
                }
            }

            let _sync = frame.finish().context("Failed to finish frame")?;

            // Plugin: post_render (§13) — still inside the bind scope.
            for plugin in &mut state.plugins {
                plugin.call_post_render();
            }
        }

        // Send frame callbacks for windows and layer surfaces
        for surface in &frame_surfaces {
            send_frames_surface_tree(surface, time_ms);
        }
        for mapped in &state.layer_surfaces {
            if mapped.surface.alive() {
                send_frames_surface_tree(mapped.surface.wl_surface(), time_ms);
            }
        }
        // Frame callbacks for lock surfaces.
        for ls in &state.lock_surfaces {
            if ls.alive() {
                send_frames_surface_tree(ls.wl_surface(), time_ms);
            }
        }
        // Frame callbacks for X11 surfaces.
        let x11_wl_surfaces: Vec<_> = state
            .xwayland_surfaces
            .iter()
            .filter_map(|s| s.wl_surface())
            .collect();
        for wl in &x11_wl_surfaces {
            send_frames_surface_tree(wl, time_ms);
        }

        // Drain wp_presentation feedback callbacks for all frame surfaces.
        {
            use smithay::wayland::compositor::with_states;
            use smithay::wayland::presentation::{PresentationFeedbackCachedState, Refresh};
            use std::time::Duration;
            use wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;

            let pres_time = Duration::from(state.clock.now());
            let refresh = Refresh::fixed(Duration::from_nanos(1_000_000_000 / 60));
            let seq = state.frame_seq;
            let output = state.output.clone();

            for surface in &frame_surfaces {
                let feedbacks = with_states(surface, |states| {
                    std::mem::take(
                        &mut states
                            .cached_state
                            .get::<PresentationFeedbackCachedState>()
                            .current()
                            .callbacks,
                    )
                });
                for feedback in feedbacks {
                    feedback.presented(
                        &output,
                        pres_time,
                        refresh,
                        seq,
                        wp_presentation_feedback::Kind::Vsync,
                    );
                }
            }
            state.frame_seq = state.frame_seq.wrapping_add(1);
        }

        // 4. Accept new Wayland clients
        if let Some(stream) = listener.accept().context("Accept error")? {
            tracing::debug!("New Wayland client connected");
            match display
                .handle()
                .insert_client(stream, Arc::new(ClientData::default()))
            {
                Ok(client) => clients.push(client),
                Err(e) => tracing::error!("Failed to insert client: {e}"),
            }
        }

        // 5. Dispatch + flush Wayland clients
        display
            .dispatch_clients(&mut state)
            .context("dispatch_clients error")?;
        display.flush_clients().context("flush_clients error")?;

        // 6. Submit frame
        backend
            .submit(Some(&[screen_rect]))
            .map_err(|e| anyhow::anyhow!("Failed to submit frame: {e:?}"))?;

        // Plugin: frame_end (§13)
        for plugin in &mut state.plugins {
            plugin.call_frame_end();
        }

        // 7a. Idle tick (§8.8)
        state.tick_idle();

        // 7. Periodic config hot-reload check (§2b)
        if last_config_check.elapsed() >= CONFIG_CHECK_INTERVAL {
            last_config_check = Instant::now();
            state.reload_config_if_changed(false);
        }

        // 7b. Periodic session autosave (§22.1)
        {
            let interval_s = state.config.session.autosave_interval_s;
            if interval_s > 0
                && !state.safe_mode
                && state.last_session_save.elapsed().as_secs() >= u64::from(interval_s)
            {
                crate::session::save(&state);
                state.last_session_save = Instant::now();
            }
        }

        // 8. IPC
        {
            let mut ipc = state.ipc.take();
            if let Some(ref mut server) = ipc {
                server.accept_event_subscribers();
                for _ in 0..16 {
                    if !server.try_handle_one(|cmd| commands::handle(cmd, &mut state)) {
                        break;
                    }
                }
                let events = std::mem::take(&mut state.pending_events);
                server.flush_events(&events);
            }
            state.ipc = ipc;
        }
    }

    // ── Save session state on clean exit (§22.1) ──────────────────────────────
    crate::session::save(&state);

    Ok(())
}

fn handle_winit_event(event: WinitEvent, state: &mut GlobalState) {
    match event {
        WinitEvent::Resized { size, .. } => {
            tracing::debug!("Window resized: {}x{}", size.w, size.h);
        }
        WinitEvent::Input(input_event) => {
            handle_input(input_event, state);
        }
        WinitEvent::CloseRequested => {
            tracing::info!("Close requested");
        }
        WinitEvent::Redraw | WinitEvent::Focus(_) => {}
    }
}

fn handle_input(event: InputEvent<smithay::backend::winit::WinitInput>, state: &mut GlobalState) {
    // Any input resets the idle clock and notifies ext-idle-notify-v1 clients.
    state.reset_idle();
    let seat = state.seat.clone();
    if let Some(notifier) = &mut state.idle_notifier_state {
        notifier.notify_activity(&seat);
    }

    match event {
        // ── Keyboard ─────────────────────────────────────────────────────────
        InputEvent::Keyboard { event } => {
            let key_state = event.state();
            if let Some(keyboard) = state.seat.get_keyboard() {
                let action = keyboard.input::<Option<Action>, _>(
                    state,
                    event.key_code(),
                    key_state,
                    0.into(),
                    event.time_msec(),
                    |data, modifiers, handle| {
                        let sym = handle.modified_sym().raw();

                        // ── Pointer-constraint break key (§8.11) ───────────────
                        // Hardcoded — cannot be overridden or intercepted by apps.
                        if key_state == KeyState::Pressed
                            && let Some((break_sym, break_mask)) = data.constraint_break_key
                        {
                            let mask: u32 =
                                (if modifiers.logo  { 1u32 } else { 0 })
                                | (if modifiers.alt  { 2 } else { 0 })
                                | (if modifiers.ctrl { 4 } else { 0 })
                                | (if modifiers.shift{ 8 } else { 0 });
                            if sym == break_sym && mask == break_mask {
                                // Deactivate all pointer constraints for the focused surface.
                                if let Some(surface) = data.grid.focused_surface()
                                    && let Some(pointer) = data.seat.get_pointer()
                                {
                                    use smithay::wayland::pointer_constraints::with_pointer_constraint;
                                    with_pointer_constraint(surface, &pointer, |c| {
                                        if let Some(c) = c {
                                            c.deactivate();
                                        }
                                    });
                                }
                                return FilterResult::Intercept(None);
                            }
                        }

                        // ── Key-release: check bind_release table ──────────────
                        if key_state == KeyState::Released {
                            data.held_keys.remove(&sym);
                            // Exit stack-peek on any key release when peeking.
                            if data.is_stack_peeking {
                                data.is_stack_peeking = false;
                                return FilterResult::Intercept(None);
                            }
                            if let Some(action) = data.release_table.lookup(sym, modifiers) {
                                return FilterResult::Intercept(Some(action));
                            }
                            return FilterResult::Forward;
                        }

                        // ── Key-repeat suppression ────────────────────────────
                        // Track held keys; OS repeat fires KeyState::Pressed again.
                        // Only dispatch if the bind allows repeat (repeat=true).
                        let is_repeat = data.held_keys.contains(&sym);
                        if is_repeat {
                            let allow = data.keybind_table.lookup_flags(sym, modifiers)
                                .map(|(_, _, _, _, repeat)| repeat)
                                .unwrap_or(false);
                            if !allow {
                                return FilterResult::Forward;
                            }
                        } else {
                            data.held_keys.insert(sym);
                        }

                        // ── Overview mode: arrow/Enter/Escape/Tab/Space navigate ─
                        if data.is_overview && data.active_submap.is_none() {
                            match sym {
                                XKB_KEY_LEFT  => return FilterResult::Intercept(Some(Action::OverviewFocusLeft)),
                                XKB_KEY_RIGHT => return FilterResult::Intercept(Some(Action::OverviewFocusRight)),
                                XKB_KEY_UP    => return FilterResult::Intercept(Some(Action::OverviewFocusUp)),
                                XKB_KEY_DOWN  => return FilterResult::Intercept(Some(Action::OverviewFocusDown)),
                                XKB_KEY_RETURN | XKB_KEY_KP_ENTER => {
                                    return FilterResult::Intercept(Some(Action::OverviewActivate));
                                }
                                XKB_KEY_ESCAPE => {
                                    // Cancel grab if active, otherwise toggle overview off.
                                    if data.overview_grabbed_window.is_some() {
                                        return FilterResult::Intercept(Some(Action::OverviewCancelGrab));
                                    }
                                    return FilterResult::Intercept(Some(Action::OverviewToggle));
                                }
                                XKB_KEY_TAB => {
                                    if modifiers.shift {
                                        return FilterResult::Intercept(Some(Action::OverviewWindowPrev));
                                    } else {
                                        return FilterResult::Intercept(Some(Action::OverviewWindowNext));
                                    }
                                }
                                XKB_KEY_SPACE => {
                                    if data.overview_grabbed_window.is_some() {
                                        return FilterResult::Intercept(Some(Action::OverviewDropWindow));
                                    } else {
                                        return FilterResult::Intercept(Some(Action::OverviewGrabWindow));
                                    }
                                }
                                _ => {} // fall through: let $mod+o and other binds still fire
                            }
                        }

                        // ── Submap-aware lookup ───────────────────────────────
                        if let Some(submap_name) = data.active_submap.clone()
                            && let Some(sub) = data.submap_tables.get(&submap_name)
                        {
                            if sub.is_exit_key(sym) {
                                return FilterResult::Intercept(Some(Action::SubmapReset));
                            }
                            match sub.lookup(sym, modifiers) {
                                Some(action) => {
                                    if sub.exit_after_action {
                                        return FilterResult::Intercept(Some(
                                            Action::SubmapActionThenReset(Box::new(action)),
                                        ));
                                    }
                                    return FilterResult::Intercept(Some(action));
                                }
                                None => {
                                    if sub.exit_on_unhandled {
                                        return FilterResult::Intercept(Some(Action::SubmapReset));
                                    }
                                    return FilterResult::Intercept(None);
                                }
                            }
                        }

                        // ── Keyboard shortcuts inhibit ────────────────────────
                        // If the focused surface has an active inhibitor (game/VM),
                        // pass all keys through without compositor keybind processing.
                        if data.seat.keyboard_shortcuts_inhibited() {
                            return FilterResult::Forward;
                        }

                        // ── Normal keybind lookup ─────────────────────────────
                        match data.keybind_table.lookup_flags(sym, modifiers) {
                            Some((action, locked, _global, passthrough, _repeat)) => {
                                // When screen is locked, only dispatch locked binds.
                                if data.is_locked && !locked {
                                    return FilterResult::Forward;
                                }
                                if passthrough {
                                    // Run the action now inside the filter, then forward
                                    // the key to the focused client (passthrough = true).
                                    if dispatcher::dispatch(action, data) {
                                        data.should_exit = true;
                                    }
                                    FilterResult::Forward
                                } else {
                                    FilterResult::Intercept(Some(action))
                                }
                            }
                            None => FilterResult::Forward,
                        }
                    },
                );
                if let Some(Some(action)) = action
                    && dispatcher::dispatch(action, state)
                {
                    state.should_exit = true;
                }
                // Detect XKB layout changes (e.g. Ctrl+Shift group switch).
                // Layout name is only meaningful after the modifier state updates,
                // so we check after input() returns.
                if key_state == smithay::backend::input::KeyState::Pressed
                    && let Some(kb) = state.seat.get_keyboard()
                {
                    let layout_now = kb.with_xkb_state(state, |ctx| {
                        let xkb = ctx.xkb().lock().unwrap();
                        xkb.layout_name(xkb.active_layout()).to_string()
                    });
                    let prev = state.current_keyboard_layout.clone();
                    if !prev.is_empty() && layout_now != prev {
                        state.pending_events.push(
                            crate::ipc::events::Event::KeyboardLayoutChanged {
                                device: "seat0".into(),
                                layout: layout_now.clone(),
                            },
                        );
                    }
                    state.current_keyboard_layout = layout_now;
                }
            }
        }

        // ── Pointer motion ────────────────────────────────────────────────────
        InputEvent::PointerMotionAbsolute { event } => {
            let x = event.x_transformed(state.grid.output_w);
            let y = event.y_transformed(state.grid.output_h);
            let prev_pos = state.cursor_pos;
            state.cursor_pos = (x, y);

            // Hot-corner detection (§7.2): enter overview on dwell.
            if !state.is_overview && state.drag.is_none() {
                let ow = state.grid.output_w;
                let oh = state.grid.output_h;
                let corner = state.config.overview.hot_corner.clone();
                let delay_ms = state.config.overview.hot_corner_delay_ms as u128;
                if is_in_hot_corner(x, y, ow, oh, &corner) {
                    if let Some(entered_at) = state.hot_corner_entered {
                        if entered_at.elapsed().as_millis() >= delay_ms {
                            state.hot_corner_entered = None;
                            dispatcher::dispatch(Action::OverviewToggle, state);
                        }
                    } else {
                        state.hot_corner_entered = Some(std::time::Instant::now());
                    }
                } else {
                    state.hot_corner_entered = None;
                }
            }

            // Update overview drag target
            if state.overview_drag.is_some() {
                let target = overview_thumbnail_at(state, x, y);
                if let Some(ref mut d) = state.overview_drag {
                    d.cursor = (x, y);
                    d.target_ws = target;
                }
            }

            // Apply drag if active
            if let Some(drag_clone) = state.drag.clone() {
                let dx = x - drag_clone.start_cursor.0;
                let dy = y - drag_clone.start_cursor.1;
                let drag_dist = (dx * dx + dy * dy).sqrt();
                let id = drag_clone.window_id;

                // Unsnap: tiled window promoted to floating once drag exceeds threshold
                if drag_clone.started_tiled {
                    let threshold = state.config.windows.unsnap_threshold_px as f64;
                    if drag_dist >= threshold {
                        let is_still_tiled = state
                            .grid
                            .windows
                            .get(&id)
                            .map(|w| w.current_state == WindowState::Tiled)
                            .unwrap_or(false);
                        if is_still_tiled {
                            // Center the floating window under the cursor
                            if let Some(rect) = state.grid.compute_rect(id)
                                && let Some(w) = state.grid.windows.get_mut(&id)
                            {
                                w.floating_geom = rect;
                            }
                            state.grid.focused_window = Some(id);
                            state.grid.toggle_floating();
                            // Update drag start so the window feels anchored to cursor
                            if let Some(ref mut d) = state.drag {
                                d.start_cursor = (x, y);
                                d.started_tiled = false;
                                if let Some(w) = state.grid.windows.get(&id) {
                                    d.start_x = w.floating_geom.x;
                                    d.start_y = w.floating_geom.y;
                                }
                            }
                        }
                    }
                    // Don't move the window while it's still tiled
                    let still_tiled = state
                        .grid
                        .windows
                        .get(&id)
                        .map(|w| w.current_state == WindowState::Tiled)
                        .unwrap_or(false);
                    if still_tiled {
                        return;
                    }
                }

                // Re-read drag (may have been mutated during unsnap)
                if let Some(drag) = state.drag.clone() {
                    let dx = x - drag.start_cursor.0;
                    let dy = y - drag.start_cursor.1;
                    if let Some(w) = state.grid.windows.get_mut(&id) {
                        match drag.kind {
                            DragKind::Move => {
                                w.floating_geom.x = drag.start_x + dx as i32;
                                w.floating_geom.y = drag.start_y + dy as i32;
                            }
                            DragKind::Resize => {
                                w.floating_geom.w = (drag.start_w + dx as i32).max(100);
                                w.floating_geom.h = (drag.start_h + dy as i32).max(60);
                            }
                        }
                        state.grid.send_configure(id);
                    }

                    // Edge snap preview (move drags only)
                    if drag.kind == DragKind::Move && state.config.windows.edge_snap {
                        let threshold = state.config.windows.edge_snap_threshold_px as i32;
                        let preview = snap_slot_for_cursor(
                            x,
                            y,
                            state.grid.output_w,
                            state.grid.output_h,
                            threshold,
                        );
                        if let Some(ref mut d) = state.drag {
                            d.snap_preview = preview;
                        }
                    }
                }
            } else {
                // Update keyboard focus based on cursor position
                let hit = state.grid.window_at(x, y);
                if let Some(id) = hit
                    && state.config.input.follow_mouse != crate::config::types::FollowMouse::Off
                {
                    update_keyboard_focus_to(state, Some(id));
                }
            }

            // Forward motion to Wayland clients via the seat pointer.
            if let Some(pointer) = state.seat.get_pointer() {
                let pointer = pointer.clone();
                let focus = pointer_focus_for_cursor(state, x, y);
                let time_ms = event.time_msec();
                let serial = SERIAL_COUNTER.next_serial();

                // Compute relative delta from previous position.
                let delta: Point<f64, Logical> = Point::from((x - prev_pos.0, y - prev_pos.1));

                pointer.motion(
                    state,
                    focus.clone(),
                    &MotionEvent {
                        location: Point::from((x, y)),
                        serial,
                        time: time_ms,
                    },
                );
                // Send relative motion for apps that requested zwp_relative_pointer_v1.
                pointer.relative_motion(
                    state,
                    focus,
                    &RelativeMotionEvent {
                        delta,
                        delta_unaccel: delta,
                        utime: time_ms as u64 * 1_000,
                    },
                );
                pointer.frame(state);
            }
        }

        // ── Pointer buttons ───────────────────────────────────────────────────
        InputEvent::PointerButton { event } => {
            let button = event.button_code();
            let pressed = event.state() == ButtonState::Pressed;

            let mod_held = state
                .seat
                .get_keyboard()
                .map(|kb| {
                    let mods = kb.modifier_state();
                    match state.config.input.mod_key.as_str() {
                        "Alt" | "Mod1" => mods.alt,
                        "Ctrl" | "Control" => mods.ctrl,
                        "Shift" => mods.shift,
                        _ => mods.logo,
                    }
                })
                .unwrap_or(false);

            // Overview mode: left click on a window starts a drag; click on
            // background activates that workspace and exits overview.
            if pressed && button == BTN_LEFT && state.is_overview
                && state.overview_anim.as_ref().map(|a| a.overview_progress()).unwrap_or(1.0) > 0.8
            {
                let (cx, cy) = state.cursor_pos;
                // Check if click lands on a window inside any thumbnail.
                if let Some((win_id, from_ws)) = overview_window_at(state, cx, cy) {
                    state.overview_drag = Some(crate::state::OverviewDragState {
                        window_id: win_id,
                        from_ws,
                        cursor: (cx, cy),
                        target_ws: Some(from_ws),
                    });
                    return;
                }
                // Click on thumbnail background → exit and switch workspace.
                let ow = state.grid.output_w;
                let oh = state.grid.output_h;
                let cols = state.grid.cols;
                let rows = state.grid.rows;
                for ov_row in 0..rows {
                    for ov_col in 0..cols {
                        let thumb = layout::overview_thumbnail_rect(
                            ov_col,
                            ov_row,
                            cols,
                            rows,
                            ow,
                            oh,
                            OV_GAP_PX,
                            OV_MARGIN_PX,
                        );
                        if (cx as i32) >= thumb.x
                            && (cx as i32) < thumb.x + thumb.w
                            && (cy as i32) >= thumb.y
                            && (cy as i32) < thumb.y + thumb.h
                        {
                            dispatcher::dispatch(Action::OverviewToggle, state);
                            dispatcher::dispatch(Action::WorkspaceTo(ov_col, ov_row), state);
                            return;
                        }
                    }
                }
                // Click outside all thumbnails: consume.
                return;
            }

            // Release overview drag.
            if !pressed && button == BTN_LEFT {
                if let Some(drag) = state.overview_drag.take() {
                    let target = drag.target_ws.unwrap_or(drag.from_ws);
                    if target == drag.from_ws {
                        // No cross-workspace move: activate that workspace.
                        dispatcher::dispatch(Action::OverviewToggle, state);
                        dispatcher::dispatch(Action::WorkspaceTo(target.0, target.1), state);
                    } else {
                        state.grid.focused_window = Some(drag.window_id);
                        state.grid.move_window_to(target);
                    }
                    return;
                }
            }

            if pressed {
                let (x, y) = state.cursor_pos;

                // Check [[bind]] mouse entries (§9.3) first — user-defined binds take priority.
                if let Some(kb) = state.seat.get_keyboard() {
                    let mods = kb.modifier_state();
                    if let Some(action) = state.keybind_table.lookup_button(button, &mods)
                        && dispatcher::dispatch(action, state)
                    {
                        state.should_exit = true;
                    }
                    // Don't consume the event — still forward to clients below.
                }

                if mod_held && (button == BTN_LEFT || button == BTN_RIGHT) {
                    // $mod + drag: move/resize floating, or unsnap tiled window
                    if let Some(id) = state.grid.window_at(x, y)
                        && let Some(w) = state.grid.windows.get(&id)
                    {
                        let is_tiled = w.current_state == WindowState::Tiled;
                        let is_floating = w.current_state == WindowState::Floating;
                        if is_floating || (is_tiled && button == BTN_LEFT) {
                            let geom = w.floating_geom;
                            let started_tiled = is_tiled;
                            state.drag = Some(DragState {
                                window_id: id,
                                kind: if button == BTN_LEFT {
                                    DragKind::Move
                                } else {
                                    DragKind::Resize
                                },
                                start_cursor: (x, y),
                                start_x: geom.x,
                                start_y: geom.y,
                                start_w: geom.w,
                                start_h: geom.h,
                                snap_preview: None,
                                started_tiled,
                            });
                            state.grid.set_focus(id);
                            update_keyboard_focus_to(state, Some(id));
                            return;
                        }
                    }
                }

                // Regular click: focus the window under cursor
                let hit = state.grid.window_at(x, y);
                update_keyboard_focus_to(state, hit);
            } else {
                // Release: apply edge snap if pending, then clear drag
                if let Some(ref drag) = state.drag
                    && let Some(slot) = drag.snap_preview
                {
                    // Ensure dragged window is focused so assign_slot targets it
                    let drag_id = drag.window_id;
                    state.grid.focused_window = Some(drag_id);
                    state.grid.assign_slot(slot);
                }
                state.drag = None;
            }

            // Forward button event to Wayland clients.
            if let Some(pointer) = state.seat.get_pointer() {
                let pointer = pointer.clone();
                pointer.button(
                    state,
                    &ButtonEvent {
                        button,
                        state: if pressed {
                            ButtonState::Pressed
                        } else {
                            ButtonState::Released
                        },
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                    },
                );
                pointer.frame(state);
            }
        }

        // ── Pointer axis (scroll) ─────────────────────────────────────────────
        InputEvent::PointerAxis { event } => {
            // Check Scroll:* binds (§9.3) before forwarding to clients.
            let hv = event.amount(Axis::Horizontal).unwrap_or(0.0);
            let vv = event.amount(Axis::Vertical).unwrap_or(0.0);
            if (hv != 0.0 || vv != 0.0)
                && let Some(kb) = state.seat.get_keyboard()
            {
                let mods = kb.modifier_state();
                // Dispatch vertical bind (if any).
                if vv != 0.0 {
                    let scroll_btn = if vv < 0.0 { 0x150u32 } else { 0x151 }; // Up / Down
                    if let Some(action) = state.keybind_table.lookup_button(scroll_btn, &mods)
                        && dispatcher::dispatch(action, state)
                    {
                        state.should_exit = true;
                    }
                }
                // Dispatch horizontal bind independently (trackpads can send both axes).
                if hv != 0.0 {
                    let scroll_btn = if hv < 0.0 { 0x152u32 } else { 0x153 }; // Left / Right
                    if let Some(action) = state.keybind_table.lookup_button(scroll_btn, &mods)
                        && dispatcher::dispatch(action, state)
                    {
                        state.should_exit = true;
                    }
                }
            }
            if let Some(pointer) = state.seat.get_pointer() {
                let pointer = pointer.clone();
                let mut frame = AxisFrame::new(event.time_msec()).source(event.source());

                if hv != 0.0 {
                    frame = frame.value(Axis::Horizontal, hv);
                }
                if vv != 0.0 {
                    frame = frame.value(Axis::Vertical, vv);
                }

                if let Some(d) = event.amount_v120(Axis::Horizontal)
                    && d != 0.0
                {
                    frame = frame.v120(Axis::Horizontal, d as i32);
                }
                if let Some(d) = event.amount_v120(Axis::Vertical)
                    && d != 0.0
                {
                    frame = frame.v120(Axis::Vertical, d as i32);
                }

                pointer.axis(state, frame);
                pointer.frame(state);
            }
        }

        // ── Touch ────────────────────────────────────────────────────────────
        InputEvent::TouchDown { event } => {
            if let Some(touch) = state.seat.get_touch() {
                let touch = touch.clone();
                let x = event.x_transformed(state.grid.output_w);
                let y = event.y_transformed(state.grid.output_h);
                let focus = pointer_focus_for_cursor(state, x, y);
                touch.down(
                    state,
                    focus,
                    &TouchDownEvt {
                        slot: event.slot(),
                        location: Point::from((x, y)),
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                    },
                );
            }
        }

        InputEvent::TouchMotion { event } => {
            if let Some(touch) = state.seat.get_touch() {
                let touch = touch.clone();
                let x = event.x_transformed(state.grid.output_w);
                let y = event.y_transformed(state.grid.output_h);
                touch.motion(
                    state,
                    None,
                    &TouchMotionEvt {
                        slot: event.slot(),
                        location: Point::from((x, y)),
                        time: event.time_msec(),
                    },
                );
            }
        }

        InputEvent::TouchUp { event } => {
            if let Some(touch) = state.seat.get_touch() {
                let touch = touch.clone();
                touch.up(
                    state,
                    &TouchUpEvt {
                        slot: event.slot(),
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                    },
                );
            }
        }

        InputEvent::TouchCancel { event: _ } => {
            if let Some(touch) = state.seat.get_touch() {
                let touch = touch.clone();
                touch.cancel(state);
            }
        }

        InputEvent::TouchFrame { event: _ } => {
            if let Some(touch) = state.seat.get_touch() {
                let touch = touch.clone();
                touch.frame(state);
            }
        }

        _ => {}
    }
}

/// Return the focused Wayland surface under (x, y) and its screen-space origin.
///
/// The second element of the tuple is the window's top-left corner in logical
/// output coordinates — Smithay uses that as the pointer-focus offset so that
/// each delivered `MotionEvent` carries the pointer position relative to the
/// surface origin.
fn pointer_focus_for_cursor(
    state: &GlobalState,
    x: f64,
    y: f64,
) -> Option<(WlSurface, Point<f64, Logical>)> {
    let id = state.grid.window_at(x, y)?;
    let window = state.grid.windows.get(&id)?;
    let rect = state.grid.compute_rect(id)?;
    Some((
        window.surface.wl_surface().clone(),
        Point::from((rect.x as f64, rect.y as f64)),
    ))
}

/// Update keyboard focus to a specific WindowId (or None to defocus).
fn update_keyboard_focus_to(state: &mut GlobalState, id: Option<crate::grid::window::WindowId>) {
    if let Some(keyboard) = state.seat.get_keyboard() {
        let keyboard = keyboard.clone();
        let surface = id
            .and_then(|id| state.grid.windows.get(&id))
            .map(|w| w.surface.wl_surface().clone());
        keyboard.set_focus(state, surface, 0.into());
    }
    if let Some(id) = id {
        state.grid.set_focus(id);
    }
}

/// Send `wl_surface::frame` callbacks for all surfaces in the tree.
fn send_frames_surface_tree(
    surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    time_ms: u32,
) {
    use smithay::wayland::compositor::{
        SurfaceAttributes, TraversalAction, with_surface_tree_downward,
    };

    with_surface_tree_downward(
        surface,
        (),
        |_, _, _| TraversalAction::DoChildren(()),
        |_surf, states, _| {
            for callback in states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .frame_callbacks
                .drain(..)
            {
                callback.done(time_ms);
            }
        },
        |_, _, _| true,
    );
}

/// Determine which snap slot (if any) the cursor position maps to.
///
/// Corners take priority over half-edges. Half-edges only activate within the
/// center 1/3 of the perpendicular screen dimension (per §6.12).
fn snap_slot_for_cursor(
    x: f64,
    y: f64,
    screen_w: i32,
    screen_h: i32,
    threshold: i32,
) -> Option<Slot> {
    let t = threshold as f64;
    let near_left = x < t;
    let near_right = x > (screen_w as f64 - t);
    let near_top = y < t;
    let near_bottom = y > (screen_h as f64 - t);

    // Corners (priority over half-edges)
    if near_left && near_top {
        return Some(Slot::QuarterTL);
    }
    if near_right && near_top {
        return Some(Slot::QuarterTR);
    }
    if near_left && near_bottom {
        return Some(Slot::QuarterBL);
    }
    if near_right && near_bottom {
        return Some(Slot::QuarterBR);
    }

    // Half-edges: center 1/3 of perpendicular dimension
    let y_pct = y / screen_h as f64;
    let in_center_third = (1.0 / 3.0..=2.0 / 3.0).contains(&y_pct);
    if near_left && in_center_third {
        return Some(Slot::HalfLeft);
    }
    if near_right && in_center_third {
        return Some(Slot::HalfRight);
    }

    None
}

/// Lighten an RGBA color by `amount` (clamped to 1.0 per channel).
/// Find the first window under (cx, cy) across all overview thumbnails.
/// Returns `(WindowId, workspace)` or `None`.
fn overview_window_at(
    state: &GlobalState,
    cx: f64,
    cy: f64,
) -> Option<(crate::grid::window::WindowId, (u8, u8))> {
    let ow = state.grid.output_w;
    let oh = state.grid.output_h;
    let cols = state.grid.cols;
    let rows = state.grid.rows;
    for ov_row in 0..rows {
        for ov_col in 0..cols {
            let thumb =
                layout::overview_thumbnail_rect(ov_col, ov_row, cols, rows, ow, oh, OV_GAP_PX, OV_MARGIN_PX);
            if (cx as i32) < thumb.x
                || (cx as i32) >= thumb.x + thumb.w
                || (cy as i32) < thumb.y
                || (cy as i32) >= thumb.y + thumb.h
            {
                continue;
            }
            let sx = thumb.w as f64 / ow as f64;
            let sy = thumb.h as f64 / oh as f64;
            for (win, rect) in state.grid.visible_windows_for_ws((ov_col, ov_row)) {
                let wx = thumb.x + (rect.x as f64 * sx) as i32;
                let wy = thumb.y + (rect.y as f64 * sy) as i32;
                let ww = ((rect.w as f64 * sx) as i32).max(2);
                let wh = ((rect.h as f64 * sy) as i32).max(2);
                if (cx as i32) >= wx
                    && (cx as i32) < wx + ww
                    && (cy as i32) >= wy
                    && (cy as i32) < wy + wh
                {
                    return Some((win.id, (ov_col, ov_row)));
                }
            }
        }
    }
    None
}

/// Return which workspace thumbnail (col, row) contains (cx, cy), if any.
fn overview_thumbnail_at(state: &GlobalState, cx: f64, cy: f64) -> Option<(u8, u8)> {
    let ow = state.grid.output_w;
    let oh = state.grid.output_h;
    let cols = state.grid.cols;
    let rows = state.grid.rows;
    for ov_row in 0..rows {
        for ov_col in 0..cols {
            let thumb =
                layout::overview_thumbnail_rect(ov_col, ov_row, cols, rows, ow, oh, OV_GAP_PX, OV_MARGIN_PX);
            if (cx as i32) >= thumb.x
                && (cx as i32) < thumb.x + thumb.w
                && (cy as i32) >= thumb.y
                && (cy as i32) < thumb.y + thumb.h
            {
                return Some((ov_col, ov_row));
            }
        }
    }
    None
}

fn lighten(c: [f32; 4], amount: f32) -> [f32; 4] {
    [
        (c[0] + amount).min(1.0),
        (c[1] + amount).min(1.0),
        (c[2] + amount).min(1.0),
        c[3],
    ]
}

/// Returns true if (x, y) is within the configured hot corner (§7.2).
fn is_in_hot_corner(x: f64, y: f64, w: i32, h: i32, corner: &str) -> bool {
    const THRESHOLD: f64 = 4.0;
    match corner {
        "top-left" => x < THRESHOLD && y < THRESHOLD,
        "top-right" => x > w as f64 - THRESHOLD && y < THRESHOLD,
        "bottom-left" => x < THRESHOLD && y > h as f64 - THRESHOLD,
        "bottom-right" => x > w as f64 - THRESHOLD && y > h as f64 - THRESHOLD,
        _ => false,
    }
}

/// Parse a `#rrggbb` or `#rrggbbaa` hex color string to `[f32; 4]`.
fn parse_hex_color(s: &str) -> [f32; 4] {
    let s = s.trim_start_matches('#');
    let byte = |i: usize| -> f32 {
        if s.len() >= i + 2 {
            u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0) as f32 / 255.0
        } else {
            0.0
        }
    };
    [
        byte(0),
        byte(2),
        byte(4),
        if s.len() >= 8 { byte(6) } else { 1.0 },
    ]
}
