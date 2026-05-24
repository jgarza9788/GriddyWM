use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use smithay::{
    input::{Seat, SeatState},
    output::{Mode as OutputMode, Output, PhysicalProperties, Subpixel},
    reexports::wayland_server::{Display, DisplayHandle},
    utils::{Clock, Monotonic, Transform},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        content_type::ContentTypeState,
        cursor_shape::CursorShapeManagerState,
        foreign_toplevel_list::{ForeignToplevelHandle, ForeignToplevelListState},
        fractional_scale::FractionalScaleManagerState,
        idle_inhibit::IdleInhibitManagerState,
        keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitState,
        output::OutputManagerState,
        pointer_constraints::PointerConstraintsState,
        pointer_gestures::PointerGesturesState,
        presentation::PresentationState,
        relative_pointer::RelativePointerManagerState,
        selection::{data_device::DataDeviceState, wlr_data_control::DataControlState},
        session_lock::{LockSurface, SessionLockManagerState, SessionLocker},
        shell::{
            wlr_layer::{Layer, LayerSurface, WlrLayerShellState},
            xdg::{XdgShellState, decoration::XdgDecorationState},
        },
        shm::ShmState,
        single_pixel_buffer::SinglePixelBufferState,
        viewporter::ViewporterState,
        xdg_activation::XdgActivationState,
        xwayland_shell::XWaylandShellState,
    },
    xwayland::{X11Surface, X11Wm},
};
use wayland_server::backend::ClientData as WlClientData;

use crate::animate::{MoveAnim, OverviewAnim, WindowAnim, WorkspaceSlide};
use crate::config::Config;
use crate::grid::{Grid, window::WindowId};
use crate::handlers::foreign_toplevel::WlrForeignToplevelState;
use crate::handlers::workspace_protocol::ExtWorkspaceState;
use crate::ipc::{IpcServer, events::Event};
use crate::keybind::{BindTable, SubBindTable};

// ─── Overview drag state ──────────────────────────────────────────────────────

/// Window being dragged between workspace thumbnails in overview mode.
#[derive(Debug, Clone)]
pub struct OverviewDragState {
    pub window_id: WindowId,
    /// Workspace the window came from.
    pub from_ws: (u8, u8),
    /// Current cursor position (for drag indicator rendering).
    pub cursor: (f64, f64),
    /// Workspace thumbnail under the cursor (highlighted drop target), if any.
    pub target_ws: Option<(u8, u8)>,
}

// ─── Drag state ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragKind {
    Move,
    Resize,
}

/// Active pointer-drag on a floating or tiled window.
#[derive(Debug, Clone)]
pub struct DragState {
    pub window_id: WindowId,
    pub kind: DragKind,
    /// Cursor position when the drag started.
    pub start_cursor: (f64, f64),
    /// Window geometry when the drag started.
    pub start_x: i32,
    pub start_y: i32,
    pub start_w: i32,
    pub start_h: i32,
    /// Edge-snap slot preview (floating move drags only).
    pub snap_preview: Option<crate::grid::window::Slot>,
    /// Whether the window was tiled when the drag started (triggers unsnap logic).
    pub started_tiled: bool,
}

// ─── Mapped layer surface ─────────────────────────────────────────────────────

pub struct MappedLayerSurface {
    pub surface: LayerSurface,
    pub layer: Layer,
    pub namespace: String,
}

// ─── Per-client data ──────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ClientData {
    pub compositor_state: CompositorClientState,
}

impl WlClientData for ClientData {
    fn initialized(&self, _client_id: wayland_server::backend::ClientId) {}
    fn disconnected(
        &self,
        _client_id: wayland_server::backend::ClientId,
        _reason: wayland_server::backend::DisconnectReason,
    ) {
    }
}

// ─── Global compositor state ──────────────────────────────────────────────────

pub struct GlobalState {
    // Smithay Wayland protocol state
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<GlobalState>,
    pub data_device_state: DataDeviceState,
    pub layer_shell_state: WlrLayerShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub foreign_toplevel_state: ForeignToplevelListState,
    pub output_manager_state: OutputManagerState,
    pub cursor_shape_state: CursorShapeManagerState,
    pub presentation_state: PresentationState,
    pub idle_inhibit_state: IdleInhibitManagerState,
    pub xdg_activation_state: XdgActivationState,
    pub relative_pointer_state: RelativePointerManagerState,
    pub pointer_constraints_state: PointerConstraintsState,
    pub viewporter_state: ViewporterState,
    pub single_pixel_buffer_state: SinglePixelBufferState,
    pub fractional_scale_state: FractionalScaleManagerState,
    pub content_type_state: ContentTypeState,
    pub data_control_state: DataControlState,

    /// pointer-gestures-unstable-v1 (swipe, pinch, hold gestures for clients).
    pub pointer_gestures_state: PointerGesturesState,

    /// keyboard-shortcuts-inhibit-unstable-v1 (games / VMs that need all keys).
    pub keyboard_shortcuts_inhibit_state: KeyboardShortcutsInhibitState,

    /// ext-session-lock-v1 protocol state.
    pub session_lock_state: SessionLockManagerState,
    /// Active lock surfaces (one per output).
    pub lock_surfaces: Vec<LockSurface>,
    /// Pending lock confirmation; held until at least one lock surface covers an output.
    pub pending_lock_confirmation: Option<SessionLocker>,

    /// xwayland-shell-v1 protocol state.
    pub xwayland_shell_state: XWaylandShellState,
    /// Running X11 window manager (None until XWayland is ready).
    pub x11_wm: Option<X11Wm>,
    /// Currently mapped X11 surfaces.
    pub xwayland_surfaces: Vec<X11Surface>,
    /// XWayland Wayland client (consumed when XWayland becomes ready).
    pub xwayland_client: Option<wayland_server::Client>,

    /// Per-window foreign toplevel handles (for `ext-foreign-toplevel-list-v1`).
    pub toplevel_handles: HashMap<WindowId, ForeignToplevelHandle>,

    /// wlr-foreign-toplevel-management-unstable-v1 state.
    pub wlr_toplevel_state: WlrForeignToplevelState,

    /// ext-workspace-v1 state.
    pub ext_workspace_state: ExtWorkspaceState,

    /// ext-idle-notify-v1 state (None until initialized in the backend).
    pub idle_notifier_state: Option<smithay::wayland::idle_notify::IdleNotifierState<GlobalState>>,

    /// Named special workspaces (scratchpad) — maps name → list of WindowIds.
    pub special_workspaces: HashMap<String, Vec<crate::grid::window::WindowId>>,
    /// Name of the currently visible special workspace, if any.
    pub active_special: Option<String>,

    /// Number of active idle-inhibit surfaces (0 = allow idle).
    pub idle_inhibit_count: usize,

    /// Monotonically incrementing frame sequence number (for presentation-time).
    pub frame_seq: u64,

    // Advertised output (single monitor for now)
    pub output: Output,

    // Mapped layer surfaces (wlr-layer-shell)
    pub layer_surfaces: Vec<MappedLayerSurface>,

    // Input seat
    pub seat: Seat<GlobalState>,

    // Grid: workspace layout, window registry, slot assignment
    pub grid: Grid,

    // Keybind lookup table (built from config or defaults)
    pub keybind_table: BindTable,

    // Gesture bind table (§9.4)
    pub gesture_table: crate::keybind::GestureTable,

    // Submap tables indexed by name (§8.7)
    pub submap_tables: HashMap<String, SubBindTable>,
    /// Currently active submap name, if any.
    pub active_submap: Option<String>,

    // Config
    pub config: Config,

    // Per-output monitor config (§8.6).
    pub monitors_config: crate::config::MonitorsConfig,

    // Hot-reload (§2b): path + mtime of the loaded config file.
    pub config_path: Option<PathBuf>,
    pub config_mtime: Option<SystemTime>,

    // Wayland display handle (for inserting clients)
    pub display_handle: DisplayHandle,

    // Clock for timestamps
    pub clock: Clock<Monotonic>,

    // Set to true by the Quit action to signal the main loop to exit.
    pub should_exit: bool,

    // IPC server (dual socket, §12). Held as Option so the main loop can
    // temporarily take it out to call handlers with &mut GlobalState.
    pub ipc: Option<IpcServer>,

    // Events accumulated during one loop tick; drained into the IPC server.
    pub pending_events: Vec<Event>,

    /// Current cursor position in logical pixels.
    pub cursor_pos: (f64, f64),

    /// Active pointer-drag operation (floating window move/resize).
    pub drag: Option<DragState>,

    /// Active workspace-slide animation (§7.1).
    pub slide_anim: Option<WorkspaceSlide>,

    /// Per-window open/fade-in animations keyed by WindowId.
    pub open_anims: HashMap<WindowId, WindowAnim>,

    /// Per-window slot/position transition animations keyed by WindowId.
    pub move_anims: HashMap<WindowId, MoveAnim>,

    /// Whether overview mode is active (§7.2).
    pub is_overview: bool,
    /// Which workspace thumbnail has keyboard focus in overview (§7.2).
    pub overview_focused: (u8, u8),
    /// In-progress overview zoom animation (open or close).
    pub overview_anim: Option<OverviewAnim>,
    /// Window being dragged between thumbnails in overview mode.
    pub overview_drag: Option<OverviewDragState>,

    /// Timestamp of the last user input event (keyboard/pointer/touch).
    pub last_input_instant: std::time::Instant,
    /// Per-timeout fired state: true = on_timeout has been executed, awaiting resume.
    pub idle_timers_fired: Vec<bool>,

    /// Whether the minimap overlay is visible.
    pub minimap_visible: bool,

    /// Runtime workspace-sync flag (§8.x). When true all outputs track the same
    /// focused workspace; when false each output is independent.
    pub workspace_synced: bool,

    /// Pending spawn-hint for the next new window (set by spawn-* dispatchers).
    pub spawn_hint: Option<crate::grid::PlacementHints>,

    /// When the cursor entered the configured hot corner (for dwell detection).
    pub hot_corner_entered: Option<std::time::Instant>,

    /// Whether the screen is locked via ext-session-lock-v1.
    pub is_locked: bool,

    /// Workspace templates loaded from `workspace-templates.toml`.
    pub templates: crate::config::WorkspaceTemplatesConfig,
    /// Workspaces that have already had their template applied.
    pub applied_templates: std::collections::HashSet<(u8, u8)>,

    // ── Overview navigation (§7.2) ────────────────────────────────────────────
    /// Index into `visible_windows_for_ws(overview_focused)` of the Tab-selected window.
    pub overview_window_idx: usize,

    /// Window currently "picked up" via Space in overview (§7.2 grab-window).
    pub overview_grabbed_window: Option<WindowId>,

    /// Original workspace of the grabbed window (for Escape cancel).
    pub overview_grab_origin: Option<(u8, u8)>,

    // ── Plugin ABI (§13) ─────────────────────────────────────────────────────
    /// Dynamically-loaded compositor plugins (requires `plugin-abi` feature).
    pub plugins: Vec<crate::plugins::LoadedPlugin>,

    // ── Cheatsheet popup ─────────────────────────────────────────────────────
    /// PID of the floating cheatsheet terminal, if currently open.
    pub cheatsheet_pid: Option<u32>,
    /// True when this is the very first run (no config file existed at launch).
    pub first_run: bool,

    // ── Session persistence (§22.1) ──────────────────────────────────────────
    /// Session state loaded at startup; entries consumed as windows are restored.
    pub session: Option<crate::session::SessionState>,

    /// When the session was last auto-saved (for periodic autosave, §22.1).
    pub last_session_save: std::time::Instant,

    // ── Crash recovery (§22.2) ───────────────────────────────────────────────
    /// True when started after ≥3 crashes in 30s; skips user config.
    pub safe_mode: bool,

    // ── wlr-output-management (§3) ───────────────────────────────────────────
    /// State for `wlr-output-management-unstable-v1`.
    pub wlr_output_manager_state: Option<crate::handlers::output_management::WlrOutputManagerState>,

    // ── Peek modes (§9, §6.8) ────────────────────────────────────────────────
    /// Whether transient overview-peek (hold to see) is active (§7.2).
    pub is_overview_peeking: bool,
    /// Whether stack-peek fan-out mode is active (§6.8).
    pub is_stack_peeking: bool,

    // ── Keybind release table (§9) ───────────────────────────────────────────
    /// Bind table for `[[bind_release]]` entries; looked up on key release.
    pub release_table: crate::keybind::BindTable,

    // ── Held key tracking (key repeat suppression §9) ────────────────────────
    /// Set of keysym values currently held down; used to suppress re-firing
    /// non-repeat binds on OS key-repeat events.
    pub held_keys: std::collections::HashSet<u32>,

    // ── Debug overlay (§22.14) ───────────────────────────────────────────────
    /// Whether the frame-time / surface-count debug overlay is visible.
    /// Toggled by `$mod+Shift+d` / `debug-overlay-toggle`.
    pub debug_overlay: bool,
    /// Timestamp of the most recently rendered frame, for FPS calculation.
    pub debug_last_frame: std::time::Instant,
    /// Running frame time (rolling last-N average).
    pub debug_frame_times: std::collections::VecDeque<f32>,

    // ── OSD state (§8.9) ─────────────────────────────────────────────────────
    /// When Some, the OSD is visible until this instant.
    pub osd_hide_at: Option<std::time::Instant>,
    /// What kind of OSD is showing (for multi-type stacking).
    pub osd_kind: Option<String>,

    // ── Screen shader (§11.5) ─────────────────────────────────────────────────
    /// Path to the active global post-process GLSL shader; None = passthrough.
    pub screen_shader: Option<String>,

    // ── Keyboard layout tracking (§12.2 keyboard_layout_changed event) ───────
    /// Human-readable name of the currently active XKB layout; empty before
    /// the first key event.  Changes trigger a `KeyboardLayoutChanged` event.
    pub current_keyboard_layout: String,

    // ── Notification daemon detection (§14.2) ─────────────────────────────────
    /// True once we have performed the one-shot D-Bus check for
    /// `org.freedesktop.Notifications`.  Prevents repeated checks.
    pub notification_daemon_checked: bool,

    // ── Pointer-constraint break key (§8.11) ─────────────────────────────────
    /// Parsed keysym + modifier mask for `input.pointer_constraint_break_key`.
    /// When this combination is pressed, all active pointer constraints are
    /// deactivated unconditionally.  Stored as (keysym_raw, mods_mask).
    pub constraint_break_key: Option<(u32, u32)>,
}

impl GlobalState {
    pub fn new(
        display: &Display<GlobalState>,
        config: Config,
        config_path: Option<PathBuf>,
        safe_mode: bool,
        first_run: bool,
    ) -> Result<Self> {
        let dh = display.handle();
        let clock = Clock::new();

        let compositor_state = CompositorState::new::<GlobalState>(&dh);
        let xdg_shell_state = XdgShellState::new::<GlobalState>(&dh);
        let shm_state = ShmState::new::<GlobalState>(&dh, vec![]);
        let data_device_state = DataDeviceState::new::<GlobalState>(&dh);
        let layer_shell_state = WlrLayerShellState::new::<GlobalState>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<GlobalState>(&dh);
        let foreign_toplevel_state = ForeignToplevelListState::new::<GlobalState>(&dh);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<GlobalState>(&dh);
        let cursor_shape_state = CursorShapeManagerState::new::<GlobalState>(&dh);
        // clk_id 1 = CLOCK_MONOTONIC (POSIX)
        let presentation_state = PresentationState::new::<GlobalState>(&dh, 1);
        let idle_inhibit_state = IdleInhibitManagerState::new::<GlobalState>(&dh);
        let xdg_activation_state = XdgActivationState::new::<GlobalState>(&dh);
        let relative_pointer_state = RelativePointerManagerState::new::<GlobalState>(&dh);
        let pointer_constraints_state = PointerConstraintsState::new::<GlobalState>(&dh);
        let viewporter_state = ViewporterState::new::<GlobalState>(&dh);
        let single_pixel_buffer_state = SinglePixelBufferState::new::<GlobalState>(&dh);
        let fractional_scale_state = FractionalScaleManagerState::new::<GlobalState>(&dh);
        let content_type_state = ContentTypeState::new::<GlobalState>(&dh);
        let data_control_state = DataControlState::new::<GlobalState, _>(&dh, None, |_| true);
        let pointer_gestures_state = PointerGesturesState::new::<GlobalState>(&dh);
        let keyboard_shortcuts_inhibit_state =
            KeyboardShortcutsInhibitState::new::<GlobalState>(&dh);
        let session_lock_state = SessionLockManagerState::new::<GlobalState, _>(&dh, |_| true);
        let xwayland_shell_state = XWaylandShellState::new::<GlobalState>(&dh);

        // text-input-v3 (§22.18 IME): zwp_text_input_manager_v3 registered for fcitx5/ibus.
        smithay::wayland::text_input::TextInputManagerState::new::<GlobalState>(&dh);

        // input-method-v2 (§22.18 IME): zwp_input_method_manager_v2 for IME engines.
        smithay::wayland::input_method::InputMethodManagerState::new::<GlobalState, _>(&dh, |_| {
            true
        });

        // security-context-v1 (§16): wp_security_context_manager_v1 for sandboxed clients.
        smithay::wayland::security_context::SecurityContextState::new::<GlobalState, _>(
            &dh,
            |_| true,
        );

        // virtual-keyboard-unstable-v1 (§22.9): wtype, ydotool, on-screen keyboards.
        smithay::wayland::virtual_keyboard::VirtualKeyboardManagerState::new::<GlobalState, _>(
            &dh,
            |_| true,
        );

        // tablet-v2 (§22.9): Wacom/Huion tablet tools.
        smithay::wayland::tablet_manager::TabletManagerState::new::<GlobalState>(&dh);

        // Set cursor theme/size env vars so Wayland clients pick them up.
        unsafe {
            std::env::set_var("XCURSOR_THEME", &config.theme.cursor.theme);
            std::env::set_var("XCURSOR_SIZE", config.theme.cursor.size.to_string());
        }

        // Register wlr-foreign-toplevel-management, ext-workspace, wlr-output-management,
        // and wp-tearing-control-v1.
        dh.create_global::<GlobalState, wayland_protocols_wlr::foreign_toplevel::v1::server::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1, ()>(3, ());
        dh.create_global::<GlobalState, wayland_protocols::ext::workspace::v1::server::ext_workspace_manager_v1::ExtWorkspaceManagerV1, ()>(1, ());
        let wlr_output_manager_state =
            Some(crate::handlers::output_management::WlrOutputManagerState::new(&dh));
        crate::handlers::tearing_control::TearingControlManagerState::new(&dh);
        crate::handlers::screencopy::WlrScreencopyState::new(&dh);
        crate::handlers::fifo::WpFifoManagerState::new(&dh);

        // Create the initial output and advertise wl_output to clients.
        let output = Output::new(
            "griddy-output".to_owned(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "GriddyWM".to_owned(),
                model: "Virtual".to_owned(),
            },
        );
        let mode = OutputMode {
            size: (1920, 1080).into(),
            refresh: 60_000,
        };
        output.set_preferred(mode);
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            None,
            Some((0, 0).into()),
        );
        output.create_global::<GlobalState>(&dh);

        let mut seat_state = SeatState::new();
        let seat = seat_state.new_wl_seat(&dh, "seat0");

        let grid = Grid::new(&config, 1920, 1080);

        // Compute initial overview_focused before config is moved into the struct.
        let initial_focused = (
            config.grid.default[0].min(config.grid.cols.max(1) - 1),
            config.grid.default[1].min(config.grid.rows.max(1) - 1),
        );

        let keybind_table = if config.binds.is_empty() {
            BindTable::from_defaults(&config.input.mod_key)
        } else {
            BindTable::from_config_with_mod(&config.binds, &config.input.mod_key)
        };

        let gesture_table = if config.gestures.is_empty() {
            crate::keybind::GestureTable::from_defaults()
        } else {
            crate::keybind::GestureTable::from_config(&config.gestures)
        };

        let release_table = if config.bind_releases.is_empty() {
            BindTable::default_releases(&config.input.mod_key)
        } else {
            BindTable::from_config_with_mod(&config.bind_releases, &config.input.mod_key)
        };

        // Parse pointer-constraint break key (§8.11) from config.
        let constraint_break_key =
            parse_constraint_break_key(&config.input.pointer_constraint_break_key);

        let submap_tables =
            crate::keybind::build_submap_tables(&config.submaps, &config.input.mod_key);

        // Read the initial mtime so we know when the file actually changes.
        let config_mtime = config_path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());

        // Load workspace templates from the config directory (§8.10).
        let templates = config_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(crate::config::load_templates)
            .unwrap_or_default();

        // Load monitors.toml for per-output configuration (§8.6).
        let monitors_config = config_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(crate::config::load_monitors)
            .unwrap_or_default();

        // Load plugins from plugins.toml in the config directory (§13).
        let plugins = {
            let mut loaded: Vec<crate::plugins::LoadedPlugin> = Vec::new();
            #[cfg(feature = "plugin-abi")]
            {
                let plugin_cfgs = config_path
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(crate::plugins::load_plugins_file)
                    .unwrap_or_default();
                for cfg in &plugin_cfgs {
                    match crate::plugins::load_plugin(cfg) {
                        Ok(p) => loaded.push(p),
                        Err(e) => tracing::warn!("Plugin load failed: {e}"),
                    }
                }
            }
            loaded
        };

        // Load session state for restore-on-startup (§22.1).
        let session = if safe_mode {
            None
        } else {
            crate::session::load()
        };

        // Start IPC server; export instance signature to child processes (§12).
        let instance_sig = format!("griddy_{}", std::process::id());
        unsafe {
            std::env::set_var("GRIDDY_INSTANCE_SIGNATURE", &instance_sig);
        }
        let ipc = match IpcServer::new(&instance_sig) {
            Ok(server) => {
                tracing::info!(sig = %instance_sig, "IPC server started");
                Some(server)
            }
            Err(e) => {
                tracing::warn!("IPC server failed to start: {e}");
                None
            }
        };

        let initial_screen_shader = if config.shaders.screen.is_empty() {
            None
        } else {
            Some(config.shaders.screen.clone())
        };
        let initial_workspace_synced =
            config.grid.workspace_sync == crate::config::types::WorkspaceSyncMode::Synced;
        let initial_overview = config.view.default_mode == crate::config::types::ViewMode::Overview;

        let mut griddy_state = GlobalState {
            compositor_state,
            xdg_shell_state,
            shm_state,
            seat_state,
            data_device_state,
            layer_shell_state,
            xdg_decoration_state,
            foreign_toplevel_state,
            output_manager_state,
            cursor_shape_state,
            presentation_state,
            idle_inhibit_state,
            xdg_activation_state,
            relative_pointer_state,
            pointer_constraints_state,
            viewporter_state,
            single_pixel_buffer_state,
            fractional_scale_state,
            content_type_state,
            data_control_state,
            pointer_gestures_state,
            keyboard_shortcuts_inhibit_state,
            session_lock_state,
            lock_surfaces: Vec::new(),
            pending_lock_confirmation: None,
            xwayland_shell_state,
            x11_wm: None,
            xwayland_surfaces: Vec::new(),
            xwayland_client: None,
            toplevel_handles: HashMap::new(),
            wlr_toplevel_state: WlrForeignToplevelState::new(),
            ext_workspace_state: ExtWorkspaceState::new(),
            idle_notifier_state: None,
            special_workspaces: HashMap::new(),
            active_special: None,
            idle_inhibit_count: 0,
            frame_seq: 0,
            output,
            layer_surfaces: Vec::new(),
            seat,
            grid,
            keybind_table,
            gesture_table,
            submap_tables,
            active_submap: None,
            config,
            monitors_config,
            config_path,
            config_mtime,
            display_handle: dh,
            clock,
            should_exit: false,
            ipc,
            pending_events: Vec::new(),
            cursor_pos: (0.0, 0.0),
            drag: None,
            slide_anim: None,
            open_anims: HashMap::new(),
            move_anims: HashMap::new(),
            is_overview: initial_overview,
            overview_focused: initial_focused,
            overview_anim: None,
            overview_drag: None,
            last_input_instant: std::time::Instant::now(),
            idle_timers_fired: Vec::new(),
            minimap_visible: false,
            workspace_synced: initial_workspace_synced,
            spawn_hint: None,
            wlr_output_manager_state,
            is_overview_peeking: false,
            is_stack_peeking: false,
            release_table,
            held_keys: std::collections::HashSet::new(),
            current_keyboard_layout: String::new(),
            notification_daemon_checked: false,
            constraint_break_key,
            debug_overlay: false,
            debug_last_frame: std::time::Instant::now(),
            debug_frame_times: std::collections::VecDeque::with_capacity(64),
            osd_hide_at: None,
            osd_kind: None,
            screen_shader: initial_screen_shader,
            hot_corner_entered: None,
            is_locked: false,
            templates,
            applied_templates: std::collections::HashSet::new(),
            overview_window_idx: 0,
            overview_grabbed_window: None,
            overview_grab_origin: None,
            plugins,
            session,
            last_session_save: std::time::Instant::now(),
            safe_mode,
            cheatsheet_pid: None,
            first_run,
        };

        if safe_mode {
            griddy_state.pending_events.push(Event::SafeModeEntered {
                reason: "3 crashes in 30s".into(),
            });
            // Show a persistent OSD warning (30 seconds) so the user knows why
            // the config was skipped (§22.2).
            let dur = std::time::Duration::from_millis(30_000);
            griddy_state.osd_hide_at = Some(std::time::Instant::now() + dur);
            griddy_state.osd_kind = Some("safe-mode".into());
        }

        Ok(griddy_state)
    }

    /// Recompute the grid's usable area by subtracting all exclusive zones from
    /// the full output dimensions and calling `grid.set_usable_area()`.
    pub fn recompute_usable_area(&mut self) {
        use smithay::wayland::compositor;
        use smithay::wayland::shell::wlr_layer::{Anchor, ExclusiveZone, LayerSurfaceCachedState};

        let ow = self.grid.output_w;
        let oh = self.grid.output_h;
        let (mut top, mut bottom, mut left, mut right) = (0i32, 0i32, 0i32, 0i32);

        for mapped in &self.layer_surfaces {
            let cached = compositor::with_states(mapped.surface.wl_surface(), |states| {
                *states
                    .cached_state
                    .get::<LayerSurfaceCachedState>()
                    .current()
            });
            let px = match cached.exclusive_zone {
                ExclusiveZone::Exclusive(v) => v as i32,
                _ => continue,
            };
            let anchor = cached.anchor;
            if anchor.contains(Anchor::TOP) && !anchor.contains(Anchor::BOTTOM) {
                top += px + cached.margin.top;
            } else if anchor.contains(Anchor::BOTTOM) && !anchor.contains(Anchor::TOP) {
                bottom += px + cached.margin.bottom;
            } else if anchor.contains(Anchor::LEFT) && !anchor.contains(Anchor::RIGHT) {
                left += px + cached.margin.left;
            } else if anchor.contains(Anchor::RIGHT) && !anchor.contains(Anchor::LEFT) {
                right += px + cached.margin.right;
            }
        }

        let x = left;
        let y = top;
        let w = (ow - left - right).max(1);
        let h = (oh - top - bottom).max(1);
        self.grid.set_usable_area(x, y, w, h);
    }

    /// Initialize keyboard and pointer on the seat.
    pub fn init_seat(&mut self) {
        let kb_cfg = &self.config.input.keyboard;
        self.seat
            .add_keyboard(
                smithay::input::keyboard::XkbConfig {
                    layout: &kb_cfg.layout,
                    variant: &kb_cfg.variant,
                    options: if kb_cfg.options.is_empty() {
                        None
                    } else {
                        Some(kb_cfg.options.clone())
                    },
                    ..Default::default()
                },
                kb_cfg.repeat_delay as i32,
                kb_cfg.repeat_rate as i32,
            )
            .expect("Failed to initialize keyboard");
        self.seat.add_pointer();
        self.seat.add_touch();
        tracing::info!(
            layout = kb_cfg.layout,
            repeat_rate = kb_cfg.repeat_rate,
            "Seat initialized"
        );
    }

    /// Spawn exec_once startup commands and optionally honor XDG autostart.
    pub fn run_startup_cmds(&self) {
        // Spawn wallpaper tool from theme.toml [wallpaper] (§8.5).
        self.spawn_wallpaper();

        for cmd in &self.config.startup.exec_once {
            tracing::info!("exec_once: {}", cmd);
            spawn_cmd(cmd);
        }
        // exec runs at startup AND on every config reload (unlike exec_once).
        for cmd in &self.config.startup.exec {
            tracing::info!("exec: {}", cmd);
            spawn_cmd(cmd);
        }

        // On first run, fire a notification pointing new users at the cheatsheet.
        if self.first_run {
            let mod_key = &self.config.input.mod_key;
            let body = format!(
                "Press {}+/ to see all keybinds, or run: griddyctl cheatsheet",
                mod_key
            );
            spawn_cmd(&format!(
                "notify-send -a GriddyWM -t 12000 'Welcome to GriddyWM' '{body}'"
            ));
        }

        if self.config.startup.honor_xdg_autostart {
            self.run_xdg_autostart();
        }
    }

    pub fn spawn_wallpaper(&self) {
        let wp = &self.config.theme.wallpaper;
        if wp.tool.is_empty() {
            return;
        }

        let image = shellexpand_tilde(&wp.default);
        let cmd = match wp.tool.as_str() {
            "swaybg" => {
                if image.is_empty() {
                    format!("swaybg --color '{}'", wp.bg_color)
                } else {
                    format!(
                        "swaybg --image '{}' --mode {} --color '{}'",
                        image, wp.mode, wp.bg_color
                    )
                }
            }
            "swww" => {
                if image.is_empty() {
                    return; // swww needs an image; skip
                }
                format!("swww img '{}'", image)
            }
            "hyprpaper" => "hyprpaper".into(),
            "mpvpaper" => {
                if image.is_empty() {
                    return;
                }
                format!("mpvpaper '*' '{}' --mpv-options='loop'", image)
            }
            other => other.to_owned(), // custom command passed verbatim
        };
        tracing::info!("Spawning wallpaper: {cmd}");
        spawn_cmd(&cmd);
    }

    /// Read `~/.config/autostart/*.desktop` and spawn eligible entries (§8 startup).
    pub fn run_xdg_autostart(&self) {
        let autostart_dir = dirs_xdg_autostart();
        let Ok(entries) = std::fs::read_dir(&autostart_dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };

            let mut app_type = String::new();
            let mut exec = String::new();
            let mut hidden = false;
            let mut only_show_in: Option<String> = None;
            let mut not_show_in: Option<String> = None;

            for line in content.lines() {
                if let Some(val) = line.strip_prefix("Type=") {
                    app_type = val.trim().to_owned();
                } else if let Some(val) = line.strip_prefix("Exec=") {
                    exec = val.trim().to_owned();
                } else if let Some(val) = line.strip_prefix("Hidden=") {
                    hidden = val.trim().eq_ignore_ascii_case("true");
                } else if let Some(val) = line.strip_prefix("OnlyShowIn=") {
                    only_show_in = Some(val.trim().to_owned());
                } else if let Some(val) = line.strip_prefix("NotShowIn=") {
                    not_show_in = Some(val.trim().to_owned());
                }
            }

            // Skip non-application entries, hidden entries, and DE-restricted ones.
            if app_type != "Application" || hidden || exec.is_empty() {
                continue;
            }
            if let Some(ref only) = only_show_in {
                let de_list: Vec<&str> = only.split(';').collect();
                if !de_list.iter().any(|de| de.eq_ignore_ascii_case("GriddyWM")) {
                    continue;
                }
            }
            if let Some(ref not) = not_show_in {
                let de_list: Vec<&str> = not.split(';').collect();
                if de_list.iter().any(|de| de.eq_ignore_ascii_case("GriddyWM")) {
                    continue;
                }
            }

            // Strip %f/%F/%u/%U/etc. field codes from Exec value.
            let exec_clean: String = exec
                .split_whitespace()
                .filter(|part| !part.starts_with('%'))
                .collect::<Vec<_>>()
                .join(" ");

            tracing::info!("XDG autostart: {}", exec_clean);
            spawn_cmd(&exec_clean);
        }
    }

    /// Timestamp in milliseconds for use in frame callbacks.
    pub fn time_ms(&self) -> u32 {
        self.clock.now().as_millis()
    }

    /// Apply cursor theme/size env vars from the live config (called after theme reload).
    pub fn apply_cursor_env(&self) {
        unsafe {
            std::env::set_var("XCURSOR_THEME", &self.config.theme.cursor.theme);
            std::env::set_var("XCURSOR_SIZE", self.config.theme.cursor.size.to_string());
        }
    }

    /// Check whether the config file has changed on disk. If so (or if `force`
    /// is true), attempt to reload and apply the new config non-destructively.
    pub fn reload_config_if_changed(&mut self, force: bool) {
        let Some(path) = self.config_path.clone() else {
            return;
        };

        // Check file mtime.
        let current_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

        if !force && current_mtime == self.config_mtime {
            return;
        }

        tracing::info!("Reloading config from {}", path.display());
        match crate::config::load_config(Some(&path)) {
            Ok(new_config) => {
                // Rebuild keybind table.
                self.keybind_table = if new_config.binds.is_empty() {
                    BindTable::from_defaults(&new_config.input.mod_key)
                } else {
                    BindTable::from_config_with_mod(&new_config.binds, &new_config.input.mod_key)
                };
                // Rebuild gesture table.
                self.gesture_table = if new_config.gestures.is_empty() {
                    crate::keybind::GestureTable::from_defaults()
                } else {
                    crate::keybind::GestureTable::from_config(&new_config.gestures)
                };
                // Rebuild release table.
                self.release_table = if new_config.bind_releases.is_empty() {
                    BindTable::default_releases(&new_config.input.mod_key)
                } else {
                    BindTable::from_config_with_mod(
                        &new_config.bind_releases,
                        &new_config.input.mod_key,
                    )
                };
                // Rebuild submap tables.
                self.submap_tables = crate::keybind::build_submap_tables(
                    &new_config.submaps,
                    &new_config.input.mod_key,
                );
                // Update constraint break key.
                self.constraint_break_key =
                    parse_constraint_break_key(&new_config.input.pointer_constraint_break_key);
                // Exit any active submap (bind layout may have changed).
                self.active_submap = None;
                // Apply gap + layout settings to the live grid.
                self.grid.update_from_config(&new_config);
                // Spawn exec commands on reload (§8 startup — exec reruns on reload).
                for cmd in &new_config.startup.exec {
                    tracing::info!("exec (reload): {}", cmd);
                    spawn_cmd(cmd);
                }
                self.config = new_config;
                // Update stored mtime.
                self.config_mtime = current_mtime;
                self.pending_events.push(Event::ConfigReloaded {
                    which: "all".into(),
                });
                self.show_osd("reload-ok");
                tracing::info!("Config reloaded successfully");
            }
            Err(e) => {
                tracing::warn!("Config reload failed (keeping previous): {e}");
                self.pending_events.push(Event::ConfigError {
                    message: e.to_string(),
                });
                self.show_osd("reload-err");
                // Still update the mtime so we don't spam retries on a broken file.
                self.config_mtime = current_mtime;
            }
        }
        // Re-export cursor env vars whenever theme may have changed.
        self.apply_cursor_env();
        // Re-sync idle timer count in case [idle.timeout] entries changed.
        let n = self.config.idle.timeouts.len();
        self.idle_timers_fired.resize(n, false);
    }

    /// Reload only theme.toml without rebuilding keybinds or rules.
    pub fn reload_theme(&mut self) {
        let Some(config_path) = &self.config_path else {
            return;
        };
        let theme_path = match config_path.parent() {
            Some(dir) => dir.join("theme.toml"),
            None => return,
        };
        if !theme_path.exists() {
            tracing::warn!("theme.toml not found at {}", theme_path.display());
            return;
        }
        match crate::config::load_theme_file(&theme_path) {
            Ok(theme) => {
                self.config.theme = theme;
                self.pending_events
                    .push(crate::ipc::events::Event::ConfigReloaded {
                        which: "theme".into(),
                    });
                self.pending_events
                    .push(crate::ipc::events::Event::ThemeReloaded);
                self.show_osd("reload-ok");
                tracing::info!("Theme reloaded from {}", theme_path.display());
            }
            Err(e) => {
                tracing::warn!("Failed to reload theme.toml: {e}");
                self.pending_events
                    .push(crate::ipc::events::Event::ConfigError {
                        message: e.to_string(),
                    });
                self.show_osd("reload-err");
            }
        }
    }

    /// Called on any user input to reset the idle clock and fire `on_resume`
    /// commands for any timers that had previously fired.
    /// Show the OSD for the configured duration (§8.9).
    pub fn show_osd(&mut self, kind: impl Into<String>) {
        if !self.config.theme.osd.enabled {
            return;
        }
        let kind = kind.into();
        // Check per-kind feature flags before showing.
        match kind.as_str() {
            "workspace" if !self.config.theme.osd.workspace_indicator => return,
            "submap" if !self.config.theme.osd.submap_indicator => return,
            "reload-ok" | "reload-err" if !self.config.theme.osd.reload_indicator => return,
            _ => {}
        }
        let dur = std::time::Duration::from_millis(self.config.theme.osd.duration_ms);
        self.osd_hide_at = Some(std::time::Instant::now() + dur);
        self.osd_kind = Some(kind);
    }

    pub fn reset_idle(&mut self) {
        if !self.config.idle.enabled {
            return;
        }
        let was_idle = self.idle_timers_fired.iter().any(|&f| f);
        if !was_idle {
            self.last_input_instant = std::time::Instant::now();
            return;
        }
        // Fire on_resume for all previously-fired timers (in reverse order).
        let resumes: Vec<(u64, String)> = self
            .config
            .idle
            .timeouts
            .iter()
            .enumerate()
            .filter(|(i, _)| self.idle_timers_fired.get(*i).copied().unwrap_or(false))
            .map(|(_, t)| (t.after_seconds, t.on_resume.clone()))
            .collect();
        for (secs, cmd) in resumes {
            if !cmd.is_empty() {
                exec_idle_cmd(&cmd);
            }
            self.pending_events.push(Event::IdleResume {
                after_seconds: secs,
            });
        }
        // Reset all fired flags and clock.
        for f in &mut self.idle_timers_fired {
            *f = false;
        }
        self.last_input_instant = std::time::Instant::now();
    }

    /// Called once per frame to check elapsed idle time and fire timeout commands.
    /// Returns the effective animation duration in ms, honoring
    /// `respect_prefers_reduced_motion` (§22.13): if set and
    /// `ACCESSIBILITY_PREFER_REDUCED_MOTION=1` is present in the environment,
    /// all animation durations are collapsed to 0.
    pub fn anim_duration(&self, base_ms: u64) -> u64 {
        if self.config.animations.respect_prefers_reduced_motion
            && std::env::var("ACCESSIBILITY_PREFER_REDUCED_MOTION").as_deref() == Ok("1")
        {
            return 0;
        }
        base_ms
    }

    pub fn tick_idle(&mut self) {
        if !self.config.idle.enabled {
            return;
        }
        // Honor idle-inhibit clients.
        if self.config.idle.inhibitors.honor_app_inhibits && self.idle_inhibit_count > 0 {
            return;
        }
        let elapsed_secs = self.last_input_instant.elapsed().as_secs();
        // Ensure the fired-flags vec is sized to match the current config.
        let n = self.config.idle.timeouts.len();
        if self.idle_timers_fired.len() != n {
            self.idle_timers_fired.resize(n, false);
        }
        for (i, timeout) in self.config.idle.timeouts.iter().enumerate() {
            if elapsed_secs >= timeout.after_seconds && !self.idle_timers_fired[i] {
                self.idle_timers_fired[i] = true;
                if !timeout.on_timeout.is_empty() {
                    exec_idle_cmd(&timeout.on_timeout);
                }
                self.pending_events.push(Event::IdleTimeout {
                    after_seconds: timeout.after_seconds,
                });
                tracing::debug!(after_seconds = timeout.after_seconds, "Idle timeout fired");
            }
        }
    }
}

// ─── Free helpers ─────────────────────────────────────────────────────────────

/// Execute an idle-management command. The special values `"dpms off"` and
/// `"dpms on"` are logged but no-op on the winit backend (no DRM/KMS control).
fn exec_idle_cmd(cmd: &str) {
    match cmd {
        "dpms off" => tracing::info!("DPMS off (no-op on winit backend)"),
        "dpms on" => tracing::info!("DPMS on (no-op on winit backend)"),
        other => spawn_cmd(other),
    }
}

/// Spawn a shell command string, splitting on whitespace.
/// Expand a leading `~/` to the user's home directory.
fn shellexpand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        format!("{home}/{rest}")
    } else {
        s.to_owned()
    }
}

fn spawn_cmd(cmd: &str) {
    let mut parts = cmd.split_whitespace();
    if let Some(prog) = parts.next() {
        let args: Vec<&str> = parts.collect();
        match std::process::Command::new(prog).args(&args).spawn() {
            Ok(child) => tracing::debug!("Spawned {} (pid {})", prog, child.id()),
            Err(e) => tracing::warn!("Failed to spawn {}: {}", prog, e),
        }
    }
}

/// Return the XDG autostart directory (`$XDG_CONFIG_HOME/autostart` or `~/.config/autostart`).
/// Parse `"Super+Escape"` style string (§8.11) into `(keysym_raw, mods_mask)`.
/// Returns `None` if the key name is not recognized.
fn parse_constraint_break_key(s: &str) -> Option<(u32, u32)> {
    use xkbcommon::xkb;
    let mut mods_mask: u32 = 0;
    let parts: Vec<&str> = s.split('+').collect();
    let key_name = parts.last()?;
    for mod_str in &parts[..parts.len().saturating_sub(1)] {
        match *mod_str {
            "Super" | "Logo" => mods_mask |= 1,
            "Alt" => mods_mask |= 2,
            "Ctrl" | "Control" => mods_mask |= 4,
            "Shift" => mods_mask |= 8,
            _ => {}
        }
    }
    let sym = xkb::keysym_from_name(key_name, xkb::KEYSYM_NO_FLAGS);
    if sym.raw() == 0 {
        return None;
    }
    Some((sym.raw(), mods_mask))
}

fn dirs_xdg_autostart() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("autostart")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("autostart")
    } else {
        PathBuf::from("/etc/xdg/autostart")
    }
}
