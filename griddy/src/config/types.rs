use serde::{Deserialize, Serialize};

// ─── Grid ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GridConfig {
    #[serde(default = "default_cols")]
    pub cols: u8,
    #[serde(default = "default_rows")]
    pub rows: u8,
    #[serde(default)]
    pub scope: GridScope,
    #[serde(default)]
    pub wrap_x: bool,
    #[serde(default)]
    pub wrap_y: bool,
    #[serde(default = "default_workspace")]
    pub default: [u8; 2],
    /// Whether workspace navigation on one monitor moves all monitors (§5).
    #[serde(default)]
    pub workspace_sync: WorkspaceSyncMode,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            cols: default_cols(),
            rows: default_rows(),
            scope: GridScope::PerOutput,
            wrap_x: false,
            wrap_y: false,
            default: default_workspace(),
            workspace_sync: WorkspaceSyncMode::Unsynced,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceSyncMode {
    #[default]
    Unsynced,
    Synced,
}

fn default_cols() -> u8 {
    3
}
fn default_rows() -> u8 {
    3
}
fn default_workspace() -> [u8; 2] {
    [1, 1]
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum GridScope {
    #[default]
    PerOutput,
    Global,
    GlobalSlice,
}

// ─── View ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ViewConfig {
    #[serde(default)]
    pub default_mode: ViewMode,
    #[serde(default = "default_slide_duration")]
    pub slide_duration_ms: u32,
    #[serde(default = "default_slide_easing")]
    pub slide_easing: String,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            default_mode: ViewMode::Focus,
            slide_duration_ms: default_slide_duration(),
            slide_easing: default_slide_easing(),
        }
    }
}

fn default_slide_duration() -> u32 {
    220
}
fn default_slide_easing() -> String {
    "ease-out-cubic".into()
}

// ─── Animations ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnimationsConfig {
    #[serde(default = "default_open_duration")]
    pub open_duration_ms: u32,
    #[serde(default = "default_close_duration")]
    pub close_duration_ms: u32,
    /// Policy when a new animation interrupts an in-progress one (§11.8).
    #[serde(default = "default_on_interrupt")]
    pub on_interrupt: String,
    #[serde(default = "default_crossfade_ms")]
    pub crossfade_ms: u32,
    /// Honor the `ACCESSIBILITY_PREFER_REDUCED_MOTION=1` environment variable
    /// and the GTK `gtk-enable-animations` setting (§22.13).
    /// When true and the env var is set, all animation durations are set to 0.
    #[serde(default = "default_true_anim")]
    pub respect_prefers_reduced_motion: bool,
}

impl Default for AnimationsConfig {
    fn default() -> Self {
        Self {
            open_duration_ms: default_open_duration(),
            close_duration_ms: default_close_duration(),
            on_interrupt: default_on_interrupt(),
            crossfade_ms: default_crossfade_ms(),
            respect_prefers_reduced_motion: default_true_anim(),
        }
    }
}

fn default_true_anim() -> bool {
    true
}

fn default_open_duration() -> u32 {
    160
}

// ─── Shaders ─────────────────────────────────────────────────────────────────

/// `[shaders]` config block (§11.5). Paths to GLSL fragment shader files.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ShadersConfig {
    /// Global screen post-process shader path; empty = passthrough.
    #[serde(default)]
    pub screen: String,
    /// Per-event animation shader overrides.
    #[serde(default)]
    pub open: String,
    #[serde(default)]
    pub close: String,
    #[serde(default)]
    pub r#move: String,
    #[serde(default)]
    pub resize: String,
    #[serde(default)]
    pub workspace_slide: String,
    #[serde(default)]
    pub overview_zoom: String,
}
fn default_close_duration() -> u32 {
    120
}
fn default_on_interrupt() -> String {
    "snap-then-start".into()
}
fn default_crossfade_ms() -> u32 {
    60
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    #[default]
    Focus,
    Overview,
}

// ─── Input ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InputConfig {
    #[serde(default = "default_mod_key")]
    pub mod_key: String,
    #[serde(default)]
    pub follow_mouse: FollowMouse,
    #[serde(default = "default_true")]
    pub cross_workspace_focus: bool,
    #[serde(default = "default_double_click")]
    pub double_click_ms: u32,
    #[serde(default = "default_true")]
    pub natural_scroll: bool,
    #[serde(default = "default_true")]
    pub tap_to_click: bool,
    #[serde(default)]
    pub keyboard: KeyboardConfig,
    #[serde(default)]
    pub touchpad: TouchpadConfig,
    #[serde(default = "default_constraint_break")]
    pub pointer_constraint_break_key: String,
    /// Warp cursor to focused window center on focus change (§22.5).
    #[serde(default)]
    pub warp_cursor_on_focus_change: bool,
    /// Warp cursor to focused window center on workspace switch (§22.5).
    #[serde(default)]
    pub warp_cursor_on_workspace_change: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            mod_key: default_mod_key(),
            follow_mouse: FollowMouse::Loose,
            cross_workspace_focus: true,
            double_click_ms: default_double_click(),
            natural_scroll: true,
            tap_to_click: true,
            keyboard: KeyboardConfig::default(),
            touchpad: TouchpadConfig::default(),
            pointer_constraint_break_key: default_constraint_break(),
            warp_cursor_on_focus_change: false,
            warp_cursor_on_workspace_change: false,
        }
    }
}

fn default_mod_key() -> String {
    "Super".into()
}
fn default_double_click() -> u32 {
    250
}
fn default_constraint_break() -> String {
    "Super+Escape".into()
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FollowMouse {
    Off,
    #[default]
    Loose,
    Strict,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeyboardConfig {
    #[serde(default = "default_layout")]
    pub layout: String,
    #[serde(default)]
    pub variant: String,
    #[serde(default)]
    pub options: String,
    #[serde(default = "default_repeat_rate")]
    pub repeat_rate: u32,
    #[serde(default = "default_repeat_delay")]
    pub repeat_delay: u32,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            layout: default_layout(),
            variant: String::new(),
            options: String::new(),
            repeat_rate: default_repeat_rate(),
            repeat_delay: default_repeat_delay(),
        }
    }
}

fn default_layout() -> String {
    "us".into()
}
fn default_repeat_rate() -> u32 {
    35
}
fn default_repeat_delay() -> u32 {
    400
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TouchpadConfig {
    #[serde(default = "default_true")]
    pub disable_while_typing: bool,
    #[serde(default = "default_scroll_method")]
    pub scroll_method: String,
}

impl Default for TouchpadConfig {
    fn default() -> Self {
        Self {
            disable_while_typing: true,
            scroll_method: default_scroll_method(),
        }
    }
}

fn default_scroll_method() -> String {
    "two-finger".into()
}

// ─── Windows ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowsConfig {
    #[serde(default)]
    pub on_slot_conflict: ConflictPolicy,
    #[serde(default = "default_true")]
    pub slot_adaptation: bool,
    #[serde(default = "default_true")]
    pub fullscreen_adaptation: bool,
    #[serde(default)]
    pub fullscreen_adaptation_stack_fallback: bool,
    #[serde(default = "default_true")]
    pub fullscreen_auto_restore: bool,
    #[serde(default = "default_true")]
    pub totalfullscreen_protects_workspace: bool,
    #[serde(default)]
    pub absolute_move_skip: bool,
    #[serde(default)]
    pub stack_scroll_direction: ScrollDirection,
    #[serde(default = "default_true")]
    pub focus_steals_demotes: bool,
    #[serde(default)]
    pub focus_on_close: FocusOnClosePolicy,
    #[serde(default)]
    pub min_size_overflow: MinSizeOverflow,
    #[serde(default = "default_edge_snap_threshold")]
    pub edge_snap_threshold_px: u32,
    #[serde(default = "default_unsnap_threshold")]
    pub unsnap_threshold_px: u32,
    #[serde(default = "default_true")]
    pub edge_snap: bool,
    #[serde(default = "default_true")]
    pub edge_snap_preview: bool,
    #[serde(default)]
    pub new_window: NewWindowConfig,
    #[serde(default)]
    pub floating: FloatingConfig,
}

impl Default for WindowsConfig {
    fn default() -> Self {
        Self {
            on_slot_conflict: ConflictPolicy::Stack,
            slot_adaptation: true,
            fullscreen_adaptation: true,
            fullscreen_adaptation_stack_fallback: false,
            fullscreen_auto_restore: true,
            totalfullscreen_protects_workspace: true,
            absolute_move_skip: false,
            stack_scroll_direction: ScrollDirection::Natural,
            focus_steals_demotes: true,
            focus_on_close: FocusOnClosePolicy::LastFocused,
            min_size_overflow: MinSizeOverflow::Float,
            edge_snap_threshold_px: default_edge_snap_threshold(),
            unsnap_threshold_px: default_unsnap_threshold(),
            edge_snap: true,
            edge_snap_preview: true,
            new_window: NewWindowConfig::default(),
            floating: FloatingConfig::default(),
        }
    }
}

fn default_edge_snap_threshold() -> u32 {
    24
}
fn default_unsnap_threshold() -> u32 {
    40
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    #[default]
    Stack,
    Swap,
    KickToFloating,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ScrollDirection {
    #[default]
    Natural,
    Inverted,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum FocusOnClosePolicy {
    #[default]
    LastFocused,
    StackNext,
    SlotNeighbor,
    None,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MinSizeOverflow {
    #[default]
    Float,
    Clip,
    Ignore,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NewWindowConfig {
    #[serde(default = "default_transient")]
    pub transient: String,
    #[serde(default = "default_same_app")]
    pub same_app: String,
    #[serde(default = "default_same_app")]
    pub default: String,
    #[serde(default = "default_true")]
    pub focus_on_open: bool,
    #[serde(default = "default_true")]
    pub focus_steal_prevention: bool,
}

impl Default for NewWindowConfig {
    fn default() -> Self {
        Self {
            transient: default_transient(),
            same_app: default_same_app(),
            default: default_same_app(),
            focus_on_open: true,
            focus_steal_prevention: true,
        }
    }
}

fn default_transient() -> String {
    "floating-on-parent".into()
}
fn default_same_app() -> String {
    "next-empty-slot".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FloatingConfig {
    #[serde(default = "default_float_size")]
    pub default_size: [u32; 2],
    #[serde(default = "default_true")]
    pub center_on_open: bool,
    #[serde(default = "default_true")]
    pub above_total_fullscreen: bool,
}

impl Default for FloatingConfig {
    fn default() -> Self {
        Self {
            default_size: default_float_size(),
            center_on_open: true,
            above_total_fullscreen: true,
        }
    }
}

fn default_float_size() -> [u32; 2] {
    [800, 600]
}

// ─── Fullscreen ───────────────────────────────────────────────────────────────

/// `[fullscreen]` section — TotalFullscreen behavior (§6.3, §8).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FullscreenConfig {
    /// Keysym name that exits TotalFullscreen (default "Escape").
    #[serde(default = "default_total_release_key")]
    pub total_release_key: String,
    /// Allow tearing_control_v1 for TotalFullscreen windows (§6.4, games).
    #[serde(default = "default_true")]
    pub allow_tearing: bool,
    /// Hide floating windows when a tearing TotalFullscreen is active.
    #[serde(default = "default_true")]
    pub tearing_hides_floating: bool,
    /// Switch to "performance" power profile via `powerprofilesctl` when
    /// TotalFullscreen is active, back to "balanced" on exit (§22.17).
    #[serde(default)]
    pub performance_on_total_fullscreen: bool,
}

impl Default for FullscreenConfig {
    fn default() -> Self {
        Self {
            total_release_key: default_total_release_key(),
            allow_tearing: true,
            tearing_hides_floating: true,
            performance_on_total_fullscreen: false,
        }
    }
}

fn default_total_release_key() -> String {
    "Escape".into()
}

// ─── Startup ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StartupConfig {
    #[serde(default)]
    pub exec_once: Vec<String>,
    #[serde(default)]
    pub exec: Vec<String>,
    #[serde(default = "default_true")]
    pub honor_xdg_autostart: bool,
}

// ─── Idle & power (§8.8) ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, rename = "timeout")]
    pub timeouts: Vec<IdleTimeoutConfig>,
    #[serde(default)]
    pub inhibitors: IdleInhibitorsConfig,
}

impl Default for IdleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeouts: Vec::new(),
            inhibitors: IdleInhibitorsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdleTimeoutConfig {
    pub after_seconds: u64,
    #[serde(default)]
    pub on_timeout: String,
    #[serde(default)]
    pub on_resume: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdleInhibitorsConfig {
    #[serde(default = "default_true")]
    pub honor_app_inhibits: bool,
    #[serde(default = "default_true")]
    pub no_idle_on_game: bool,
    #[serde(default)]
    pub no_idle_on_audio: bool,
}

impl Default for IdleInhibitorsConfig {
    fn default() -> Self {
        Self {
            honor_app_inhibits: true,
            no_idle_on_game: true,
            no_idle_on_audio: false,
        }
    }
}

// ─── XWayland ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct XWaylandConfig {
    #[serde(default = "default_true")]
    pub enable: bool,
    #[serde(default = "default_true")]
    pub hidpi: bool,
    #[serde(default = "default_scale")]
    pub scale: f64,
}

impl Default for XWaylandConfig {
    fn default() -> Self {
        Self {
            enable: true,
            hidpi: true,
            scale: default_scale(),
        }
    }
}

fn default_scale() -> f64 {
    1.0
}

// ─── Session persistence config ───────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionConfig {
    /// Automatically save session every N seconds while running (0 = disabled).
    #[serde(default = "default_autosave_s")]
    pub autosave_interval_s: u32,
    /// Restore window→workspace assignments from previous session on startup.
    #[serde(default = "default_true")]
    pub restore_on_start: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            autosave_interval_s: default_autosave_s(),
            restore_on_start: true,
        }
    }
}

fn default_autosave_s() -> u32 {
    60
}

// ─── Keybinds ─────────────────────────────────────────────────────────────────

/// One [[bind]] entry from keybinds.toml (§9).
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct BindConfig {
    /// Modifier names: "Super", "Alt", "Ctrl", "Shift".
    #[serde(default)]
    pub mods: Vec<String>,
    /// XKB key name, e.g. "h", "Return", "Left", "Tab".
    #[serde(default)]
    pub key: String,
    /// Action string, e.g. "workspace-left" or "exec kitty".
    #[serde(default)]
    pub action: String,
    /// Optional typed arguments to the action.
    #[serde(default)]
    pub args: Vec<String>,
    /// Human-readable description (not used at runtime).
    #[serde(default)]
    pub description: String,
    /// If true, action fires repeatedly while key is held.
    #[serde(default)]
    pub repeat: bool,
    /// If true, fires even when screen is locked.
    #[serde(default)]
    pub locked: bool,
    /// If true, fires even when a TotalFullscreen window has input grab.
    #[serde(default)]
    pub global: bool,
    /// If true, compositor runs the action AND forwards the keypress to the app.
    #[serde(default)]
    pub passthrough: bool,
}

// ─── Submaps (§8.7) ──────────────────────────────────────────────────────────

/// One key binding within a submap.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SubBindConfig {
    #[serde(default)]
    pub mods: Vec<String>,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Fire repeatedly while key is held.
    #[serde(default)]
    pub repeat: bool,
}

// ─── Gesture binds (§9.4) ────────────────────────────────────────────────────

/// One multi-finger swipe or pinch bind.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GestureConfig {
    /// Number of fingers (2–5).
    #[serde(default = "default_gesture_fingers")]
    pub fingers: u8,
    /// Direction: "up" | "down" | "left" | "right" | "pinch-in" | "pinch-out".
    #[serde(default)]
    pub direction: String,
    /// Action string, same as [[bind]].
    #[serde(default)]
    pub action: String,
    /// Optional typed arguments to the action.
    #[serde(default)]
    pub args: Vec<String>,
}

fn default_gesture_fingers() -> u8 {
    4
}

/// A named modal key mode.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubmapConfig {
    pub name: String,
    /// Keys that always exit the submap (without dispatching an action).
    #[serde(default)]
    pub exit_keys: Vec<String>,
    /// Exit the submap when an unrecognized key is pressed.
    #[serde(default = "default_true")]
    pub exit_on_unhandled: bool,
    /// Exit the submap after any recognized action fires (placement-submap style).
    #[serde(default)]
    pub exit_after_action: bool,
    /// The key bindings active in this submap.
    #[serde(default, rename = "bind")]
    pub binds: Vec<SubBindConfig>,
}

// ─── Overview (§7.2) ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OverviewConfig {
    /// Screen corner that triggers overview on hover — "top-left" | "top-right" |
    /// "bottom-left" | "bottom-right" | "none".
    #[serde(default = "default_hot_corner")]
    pub hot_corner: String,
    /// Milliseconds the cursor must dwell in the corner before overview fires.
    #[serde(default = "default_hot_corner_delay")]
    pub hot_corner_delay_ms: u32,
    /// Show window titles below each thumbnail window.
    #[serde(default = "default_true")]
    pub show_titles: bool,
    /// Show workspace user labels in overview thumbnails.
    #[serde(default = "default_true")]
    pub show_labels: bool,
    /// Thumbnail scale factor — 0.0 means auto-compute from grid dimensions.
    #[serde(default)]
    pub scale: f32,
    /// Gap between workspace thumbnails in overview (px).
    #[serde(default = "default_overview_gap")]
    pub gap_px: i32,
    /// Outer margin around the overview grid (px).
    #[serde(default = "default_overview_margin")]
    pub margin_px: i32,
}

impl Default for OverviewConfig {
    fn default() -> Self {
        Self {
            hot_corner: default_hot_corner(),
            hot_corner_delay_ms: default_hot_corner_delay(),
            show_titles: true,
            show_labels: true,
            scale: 0.0,
            gap_px: default_overview_gap(),
            margin_px: default_overview_margin(),
        }
    }
}

fn default_hot_corner() -> String {
    "top-left".into()
}
fn default_hot_corner_delay() -> u32 {
    150
}
fn default_overview_gap() -> i32 {
    16
}
fn default_overview_margin() -> i32 {
    32
}

// ─── Root Config ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub grid: GridConfig,
    #[serde(default)]
    pub view: ViewConfig,
    #[serde(default)]
    pub animations: AnimationsConfig,
    #[serde(default)]
    pub overview: OverviewConfig,
    #[serde(default)]
    pub input: InputConfig,
    #[serde(default)]
    pub windows: WindowsConfig,
    #[serde(default)]
    pub fullscreen: FullscreenConfig,
    #[serde(default)]
    pub startup: StartupConfig,
    #[serde(default)]
    pub idle: IdleConfig,
    #[serde(default)]
    pub xwayland: XWaylandConfig,
    #[serde(default)]
    pub session: SessionConfig,
    /// Extra env vars to set in the compositor environment.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Additional TOML files to import.
    #[serde(default)]
    pub imports: Vec<String>,
    /// Keybinds. Empty = use built-in defaults (§9.1).
    #[serde(default, rename = "bind")]
    pub binds: Vec<BindConfig>,
    /// Release-binds — fire on key release (e.g. overview-peek, stack-peek hold-to-peek).
    #[serde(default, rename = "bind_release")]
    pub bind_releases: Vec<BindConfig>,
    /// Modal key submaps (§8.7). Empty = use built-in placement+resize submaps.
    #[serde(default, rename = "submap")]
    pub submaps: Vec<SubmapConfig>,
    /// Touch/gesture binds (§9.4).
    #[serde(default, rename = "gesture")]
    pub gestures: Vec<GestureConfig>,
    /// Window rules (§10). Also auto-loaded from rules.toml.
    #[serde(default, rename = "rule")]
    pub rules: Vec<crate::config::rules::Rule>,
    /// Shader paths (§11.5).
    #[serde(default)]
    pub shaders: ShadersConfig,
    /// Theme / appearance. Also auto-loaded from theme.toml.
    #[serde(default)]
    pub theme: crate::config::theme::ThemeConfig,
    /// Preferred terminal emulator for the cheatsheet popup and other spawned UI.
    /// Empty = auto-detect (tries foot → kitty → alacritty → wezterm → xterm in order).
    #[serde(default)]
    pub terminal: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_sync_mode_default_is_unsynced() {
        let cfg = GridConfig::default();
        assert_eq!(cfg.workspace_sync, WorkspaceSyncMode::Unsynced);
    }

    #[test]
    fn workspace_sync_mode_deserializes_synced() {
        let toml = r#"
            cols = 3
            rows = 3
            workspace_sync = "synced"
        "#;
        let cfg: GridConfig = toml::from_str(toml).expect("should parse");
        assert_eq!(cfg.workspace_sync, WorkspaceSyncMode::Synced);
    }

    #[test]
    fn workspace_sync_mode_deserializes_unsynced() {
        let toml = r#"
            cols = 3
            rows = 3
            workspace_sync = "unsynced"
        "#;
        let cfg: GridConfig = toml::from_str(toml).expect("should parse");
        assert_eq!(cfg.workspace_sync, WorkspaceSyncMode::Unsynced);
    }

    #[test]
    fn grid_config_missing_workspace_sync_defaults_unsynced() {
        let toml = r#"cols = 2
rows = 2"#;
        let cfg: GridConfig = toml::from_str(toml).expect("should parse");
        assert_eq!(cfg.workspace_sync, WorkspaceSyncMode::Unsynced);
    }

    #[test]
    fn animations_config_default_durations() {
        let cfg = AnimationsConfig::default();
        assert_eq!(cfg.open_duration_ms, 160);
        assert_eq!(cfg.close_duration_ms, 120);
        assert_eq!(cfg.on_interrupt, "snap-then-start");
    }

    #[test]
    fn full_config_default_round_trips() {
        let cfg = Config::default();
        assert_eq!(cfg.grid.cols, 3);
        assert_eq!(cfg.grid.rows, 3);
        assert!(!cfg.grid.wrap_x);
        assert!(!cfg.grid.wrap_y);
        assert_eq!(cfg.grid.workspace_sync, WorkspaceSyncMode::Unsynced);
    }

    #[test]
    fn fullscreen_config_defaults() {
        let cfg = FullscreenConfig::default();
        assert_eq!(cfg.total_release_key, "Escape");
        assert!(cfg.allow_tearing);
        assert!(cfg.tearing_hides_floating);
        assert!(!cfg.performance_on_total_fullscreen);
    }

    #[test]
    fn fullscreen_config_toml_round_trip() {
        let toml = r#"
            total_release_key = "F11"
            allow_tearing = false
            performance_on_total_fullscreen = true
        "#;
        let cfg: FullscreenConfig = toml::from_str(toml).expect("should parse");
        assert_eq!(cfg.total_release_key, "F11");
        assert!(!cfg.allow_tearing);
        assert!(cfg.performance_on_total_fullscreen);
    }

    #[test]
    fn view_config_default_mode_is_focus() {
        let cfg = ViewConfig::default();
        assert_eq!(cfg.default_mode, ViewMode::Focus);
        assert_eq!(cfg.slide_duration_ms, 220);
    }

    #[test]
    fn view_config_overview_mode_parses() {
        let toml = r#"default_mode = "overview""#;
        let cfg: ViewConfig = toml::from_str(toml).expect("should parse");
        assert_eq!(cfg.default_mode, ViewMode::Overview);
    }

    #[test]
    fn startup_config_exec_fields() {
        let toml = r#"
            exec_once = ["waybar"]
            exec = ["swaybg -c '#000000'"]
        "#;
        let cfg: StartupConfig = toml::from_str(toml).expect("should parse");
        assert_eq!(cfg.exec_once, vec!["waybar"]);
        assert_eq!(cfg.exec, vec!["swaybg -c '#000000'"]);
        assert!(cfg.honor_xdg_autostart);
    }

    #[test]
    fn session_config_defaults() {
        let cfg = SessionConfig::default();
        assert_eq!(cfg.autosave_interval_s, 60);
        assert!(cfg.restore_on_start);
    }

    #[test]
    fn xwayland_config_defaults() {
        let cfg = XWaylandConfig::default();
        assert!(cfg.enable);
        assert!(cfg.hidpi);
        assert_eq!(cfg.scale, 1.0);
    }

    #[test]
    fn idle_config_defaults() {
        let cfg = IdleConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.timeouts.is_empty());
        assert!(cfg.inhibitors.honor_app_inhibits);
    }

    #[test]
    fn idle_timeout_parses() {
        let toml = r#"
            [[timeout]]
            after_seconds = 300
            on_timeout = "swaylock"
            on_resume = "notify-send resumed"
        "#;
        #[derive(serde::Deserialize)]
        struct Wrapper {
            timeout: Vec<IdleTimeoutConfig>,
        }
        let w: Wrapper = toml::from_str(toml).expect("should parse");
        assert_eq!(w.timeout[0].after_seconds, 300);
        assert_eq!(w.timeout[0].on_timeout, "swaylock");
        assert_eq!(w.timeout[0].on_resume, "notify-send resumed");
    }

    #[test]
    fn input_follow_mouse_variants() {
        for (s, expected) in [
            ("off", FollowMouse::Off),
            ("loose", FollowMouse::Loose),
            ("strict", FollowMouse::Strict),
        ] {
            let toml = format!(r#"follow_mouse = "{s}""#);
            let cfg: InputConfig = toml::from_str(&toml).expect("should parse");
            assert_eq!(cfg.follow_mouse, expected, "variant: {s}");
        }
    }

    #[test]
    fn conflict_policy_kebab_case() {
        for (s, expected) in [
            ("stack", ConflictPolicy::Stack),
            ("swap", ConflictPolicy::Swap),
            ("kick-to-floating", ConflictPolicy::KickToFloating),
        ] {
            let toml = format!(r#"on_slot_conflict = "{s}""#);
            let cfg: WindowsConfig = toml::from_str(&toml).expect("should parse");
            assert_eq!(cfg.on_slot_conflict, expected, "variant: {s}");
        }
    }
}
