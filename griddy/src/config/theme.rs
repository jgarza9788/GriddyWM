//! ThemeConfig — parsed from theme.toml (§8.5).

use serde::{Deserialize, Serialize};

// ─── Window decoration config ─────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowDecoConfig {
    /// Border thickness in pixels (0 = no border).
    #[serde(default = "default_border_px")]
    pub border_px: i32,
    /// Rounded corner radius in pixels (0 = square corners).
    #[serde(default)]
    pub rounded_px: i32,
    /// Window opacity (0.0 = transparent, 1.0 = opaque).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

impl Default for WindowDecoConfig {
    fn default() -> Self {
        Self { border_px: default_border_px(), rounded_px: 0, opacity: default_opacity() }
    }
}

fn default_border_px() -> i32 { 2 }
fn default_opacity()   -> f32  { 1.0 }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowUnfocusedDecoConfig {
    #[serde(default = "default_border_px")]
    pub border_px: i32,
    #[serde(default)]
    pub rounded_px: i32,
    #[serde(default = "default_unfocused_opacity")]
    pub opacity: f32,
    /// Inactive dim factor (0.0 = no dim, 1.0 = fully dim).
    #[serde(default = "default_inactive_dim")]
    pub inactive_dim: f32,
}

impl Default for WindowUnfocusedDecoConfig {
    fn default() -> Self {
        Self { border_px: default_border_px(), rounded_px: 0, opacity: default_unfocused_opacity(), inactive_dim: default_inactive_dim() }
    }
}

fn default_unfocused_opacity() -> f32 { 0.97 }
fn default_inactive_dim()      -> f32 { 0.08 }

// ─── Urgent window decoration ─────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowUrgentConfig {
    #[serde(default = "default_urgent_border_px")]
    pub border_px: i32,
    /// Pulse animation duration in ms; 0 = no pulse.
    #[serde(default = "default_border_pulse_ms")]
    pub border_pulse_ms: u32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

impl Default for WindowUrgentConfig {
    fn default() -> Self {
        Self {
            border_px: default_urgent_border_px(),
            border_pulse_ms: default_border_pulse_ms(),
            opacity: default_opacity(),
        }
    }
}

fn default_urgent_border_px() -> i32 { 3 }
fn default_border_pulse_ms()  -> u32  { 600 }

// ─── Floating window decoration overrides ─────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowFloatingConfig {
    #[serde(default = "default_border_px")]
    pub border_px: i32,
    #[serde(default = "default_floating_rounded_px")]
    pub rounded_corners_px: i32,
}

impl Default for WindowFloatingConfig {
    fn default() -> Self {
        Self {
            border_px: default_border_px(),
            rounded_corners_px: default_floating_rounded_px(),
        }
    }
}

fn default_floating_rounded_px() -> i32 { 12 }

// ─── Stack depth cue config (§6.8) ───────────────────────────────────────────

/// Stack depth cue ("deck of cards") configuration (§6.8).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StackedConfig {
    /// Show back-edges of windows beneath the top-of-stack window.
    #[serde(default = "default_true")]
    pub depth_cue: bool,
    /// Pixels each layer shifts right and down from the one above.
    #[serde(default = "default_depth_offset")]
    pub depth_offset_px: i32,
    /// Maximum number of layers to render depth cues for.
    #[serde(default = "default_depth_max_layers")]
    pub depth_max_layers: usize,
    /// Stack-peek display style: "cascade" | "fan" | "grid" (§6.8).
    #[serde(default = "default_peek_style")]
    pub peek_style: String,
    /// Pixel offset per layer in cascade peek mode (§6.8).
    #[serde(default = "default_peek_cascade_offset_px")]
    pub peek_cascade_offset_px: i32,
    /// Opacity of non-highlighted windows during peek (§6.8).
    #[serde(default = "default_peek_dim_unstacked")]
    pub peek_dim_unstacked: f32,
}

impl Default for StackedConfig {
    fn default() -> Self {
        Self {
            depth_cue: true,
            depth_offset_px: default_depth_offset(),
            depth_max_layers: default_depth_max_layers(),
            peek_style: default_peek_style(),
            peek_cascade_offset_px: default_peek_cascade_offset_px(),
            peek_dim_unstacked: default_peek_dim_unstacked(),
        }
    }
}

fn default_depth_offset()          -> i32   { 4 }
fn default_depth_max_layers()      -> usize { 3 }
fn default_peek_style()            -> String { "cascade".into() }
fn default_peek_cascade_offset_px() -> i32  { 28 }
fn default_peek_dim_unstacked()    -> f32   { 0.75 }

// ─── Window theme section ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WindowThemeSection {
    #[serde(default)]
    pub focused: WindowDecoConfig,
    #[serde(default)]
    pub unfocused: WindowUnfocusedDecoConfig,
    #[serde(default)]
    pub urgent: WindowUrgentConfig,
    #[serde(default)]
    pub floating: WindowFloatingConfig,
    #[serde(default)]
    pub stacked: StackedConfig,
}

// ─── Wallpaper ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WallpaperConfig {
    /// Wallpaper daemon to spawn: "swaybg" | "swww" | "mpvpaper" | "hyprpaper" | "custom"
    #[serde(default)]
    pub tool: String,
    /// Fill mode for swaybg: "fill" | "fit" | "stretch" | "center" | "tile"
    #[serde(default = "default_wallpaper_mode")]
    pub mode: String,
    /// Fallback solid color when no image is set.
    #[serde(default = "default_wallpaper_bg")]
    pub bg_color: String,
    /// Default wallpaper image path (empty = no image, use bg_color).
    #[serde(default)]
    pub default: String,
}

impl Default for WallpaperConfig {
    fn default() -> Self {
        Self {
            tool: String::new(),
            mode: default_wallpaper_mode(),
            bg_color: default_wallpaper_bg(),
            default: String::new(),
        }
    }
}

fn default_wallpaper_mode() -> String { "fill".into() }
fn default_wallpaper_bg() -> String { "#1a1b26".into() }

// ─── Top-level ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ThemeConfig {
    /// Built-in preset name to import (e.g. "catppuccin-mocha").
    #[serde(default)]
    pub import: String,

    #[serde(default)]
    pub gaps: GapsConfig,

    #[serde(default)]
    pub colors: ColorConfig,

    #[serde(default)]
    pub window: WindowThemeSection,

    #[serde(default)]
    pub cursor: CursorConfig,

    #[serde(default)]
    pub blur: BlurConfig,

    #[serde(default)]
    pub wallpaper: WallpaperConfig,

    #[serde(default)]
    pub osd: OsdConfig,
}

// ─── OSD config (§8.9) ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsdConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Position: top-left|top-center|top-right|center|bottom-left|bottom-center|bottom-right
    #[serde(default = "default_osd_position")]
    pub position: String,
    #[serde(default = "default_osd_margin")]
    pub margin_px: i32,
    #[serde(default = "default_osd_duration")]
    pub duration_ms: u64,
    #[serde(default = "default_true")]
    pub workspace_indicator: bool,
    #[serde(default = "default_true")]
    pub submap_indicator: bool,
    #[serde(default = "default_true")]
    pub reload_indicator: bool,
    #[serde(default = "default_osd_cell_px")]
    pub cell_px: i32,
    #[serde(default = "default_osd_cell_gap")]
    pub cell_gap_px: i32,
}

impl Default for OsdConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            position: default_osd_position(),
            margin_px: default_osd_margin(),
            duration_ms: default_osd_duration(),
            workspace_indicator: true,
            submap_indicator: true,
            reload_indicator: true,
            cell_px: default_osd_cell_px(),
            cell_gap_px: default_osd_cell_gap(),
        }
    }
}

fn default_osd_position() -> String { "top-center".into() }
fn default_osd_margin()   -> i32   { 24 }
fn default_osd_duration() -> u64   { 1500 }
fn default_osd_cell_px()  -> i32   { 10 }
fn default_osd_cell_gap() -> i32   { 3 }

// ─── Gaps ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GapsConfig {
    #[serde(default)]
    pub windows: WindowGapConfig,
    #[serde(default)]
    pub workspaces: WorkspaceGapConfig,
}


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowGapConfig {
    /// Gap between adjacent tiled windows.
    #[serde(default = "default_inner_px")]
    pub inner_px: i32,
    /// Gap between outermost windows and screen edge.
    #[serde(default = "default_outer_px")]
    pub outer_px: i32,
    /// Per-edge overrides (take precedence over outer_px).
    #[serde(default = "default_outer_px")]
    pub outer_top_px: i32,
    #[serde(default = "default_outer_px")]
    pub outer_bottom_px: i32,
    #[serde(default = "default_outer_px")]
    pub outer_left_px: i32,
    #[serde(default = "default_outer_px")]
    pub outer_right_px: i32,
    /// Collapse gaps to zero when only one tiled window is present.
    #[serde(default = "default_true")]
    pub smart: bool,
}

impl Default for WindowGapConfig {
    fn default() -> Self {
        Self {
            inner_px: default_inner_px(),
            outer_px: default_outer_px(),
            outer_top_px: default_outer_px(),
            outer_bottom_px: default_outer_px(),
            outer_left_px: default_outer_px(),
            outer_right_px: default_outer_px(),
            smart: true,
        }
    }
}

fn default_inner_px() -> i32 { 8 }
fn default_outer_px() -> i32 { 12 }
fn default_true() -> bool { true }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceGapConfig {
    /// Gap between workspace thumbnails in overview mode.
    #[serde(default = "default_overview_gap")]
    pub overview_gap_px: i32,
    /// Gap (in logical px) visible between workspaces during a slide.
    #[serde(default)]
    pub slide_gap_px: i32,
}

impl Default for WorkspaceGapConfig {
    fn default() -> Self {
        Self {
            overview_gap_px: default_overview_gap(),
            slide_gap_px: 0,
        }
    }
}

fn default_overview_gap() -> i32 { 20 }

// ─── Colors ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ColorConfig {
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_accent_dim")]
    pub accent_dim: String,
    #[serde(default = "default_fg")]
    pub fg: String,
    #[serde(default = "default_fg_dim")]
    pub fg_dim: String,
    #[serde(default = "default_bg")]
    pub bg: String,
    #[serde(default = "default_bg_alt")]
    pub bg_alt: String,
    #[serde(default = "default_border_idle")]
    pub border_idle: String,
    #[serde(default = "default_warn")]
    pub warn: String,
    #[serde(default = "default_danger")]
    pub danger: String,
    #[serde(default = "default_ok")]
    pub ok: String,
    #[serde(default = "default_shadow")]
    pub shadow: String,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            accent:      default_accent(),
            accent_dim:  default_accent_dim(),
            fg:          default_fg(),
            fg_dim:      default_fg_dim(),
            bg:          default_bg(),
            bg_alt:      default_bg_alt(),
            border_idle: default_border_idle(),
            warn:        default_warn(),
            danger:      default_danger(),
            ok:          default_ok(),
            shadow:      default_shadow(),
        }
    }
}

fn default_accent()      -> String { "#7aa2f7".into() }
fn default_accent_dim()  -> String { "#3d59a1".into() }
fn default_fg()          -> String { "#c0caf5".into() }
fn default_fg_dim()      -> String { "#a9b1d6".into() }
fn default_bg()          -> String { "#1a1b26".into() }
fn default_bg_alt()      -> String { "#24283b".into() }
fn default_border_idle() -> String { "#414868".into() }
fn default_warn()        -> String { "#e0af68".into() }
fn default_danger()      -> String { "#f7768e".into() }
fn default_ok()          -> String { "#9ece6a".into() }
fn default_shadow()      -> String { "#000000aa".into() }

// ─── Cursor ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CursorConfig {
    #[serde(default = "default_cursor_theme")]
    pub theme: String,
    #[serde(default = "default_cursor_size")]
    pub size: u32,
    /// Hide cursor after this many ms of inactivity (0 = never).
    #[serde(default)]
    pub inactivity_timeout_ms: u32,
    #[serde(default = "default_true")]
    pub hide_on_typing: bool,
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            theme: default_cursor_theme(),
            size: default_cursor_size(),
            inactivity_timeout_ms: 0,
            hide_on_typing: true,
        }
    }
}

fn default_cursor_theme() -> String { "default".into() }
fn default_cursor_size()  -> u32    { 24 }

// ─── Blur ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlurConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_blur_passes")]
    pub passes: u32,
    #[serde(default = "default_blur_size")]
    pub size_px: u32,
    #[serde(default = "default_blur_noise")]
    pub noise: f32,
}

impl Default for BlurConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            passes: default_blur_passes(),
            size_px: default_blur_size(),
            noise: default_blur_noise(),
        }
    }
}

fn default_blur_passes() -> u32  { 2 }
fn default_blur_size()   -> u32  { 8 }
fn default_blur_noise()  -> f32  { 0.0117 }
