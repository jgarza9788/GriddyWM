use smithay::{
    reexports::wayland_server::backend::ObjectId, wayland::shell::xdg::ToplevelSurface,
};

pub type WindowId = u64;

// ─── Slot ────────────────────────────────────────────────────────────────────

/// One of the six geometric tiled regions within a workspace (§6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    HalfLeft,
    HalfRight,
    QuarterTL,
    QuarterTR,
    QuarterBL,
    QuarterBR,
}

impl Slot {
    pub const ALL: [Slot; 6] = [
        Slot::HalfLeft,
        Slot::HalfRight,
        Slot::QuarterTL,
        Slot::QuarterTR,
        Slot::QuarterBL,
        Slot::QuarterBR,
    ];

    /// Reading-order scan: HalfLeft → HalfRight → QuarterTL → QuarterTR → QuarterBL → QuarterBR
    pub const SCAN_ORDER: [Slot; 6] = [
        Slot::HalfLeft,
        Slot::HalfRight,
        Slot::QuarterTL,
        Slot::QuarterTR,
        Slot::QuarterBL,
        Slot::QuarterBR,
    ];

    pub fn is_half(self) -> bool {
        matches!(self, Slot::HalfLeft | Slot::HalfRight)
    }

    pub fn is_quarter(self) -> bool {
        matches!(
            self,
            Slot::QuarterTL | Slot::QuarterTR | Slot::QuarterBL | Slot::QuarterBR
        )
    }

    /// Normalized center position (0.0–1.0) for spatial distance calculations.
    pub fn center_norm(self) -> (f64, f64) {
        match self {
            Slot::HalfLeft => (0.25, 0.5),
            Slot::HalfRight => (0.75, 0.5),
            Slot::QuarterTL => (0.25, 0.25),
            Slot::QuarterTR => (0.75, 0.25),
            Slot::QuarterBL => (0.25, 0.75),
            Slot::QuarterBR => (0.75, 0.75),
        }
    }
}

// ─── Window state ─────────────────────────────────────────────────────────────

/// State determines geometry and Z-order. Orthogonal to slot membership (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    Tiled,
    Floating,
    Fullscreen,
    TotalFullscreen,
}

// ─── Rect ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn center_x(&self) -> f64 {
        self.x as f64 + self.w as f64 / 2.0
    }

    pub fn center_y(&self) -> f64 {
        self.y as f64 + self.h as f64 / 2.0
    }
}

// ─── Window ───────────────────────────────────────────────────────────────────

/// A mapped xdg_toplevel with its grid assignment (§6.1).
pub struct Window {
    pub id: WindowId,
    pub app_id: String,
    pub title: String,
    /// Grid coordinates of the owning workspace.
    pub workspace: (u8, u8),
    /// What the window currently is.
    pub current_state: WindowState,
    /// What the user / rule originally asked for (may differ when adapted).
    pub requested_state: WindowState,
    /// Current slot (Some only when current_state == Tiled).
    pub current_slot: Option<Slot>,
    /// Originally requested slot (remembered for restore after adaptation).
    pub requested_slot: Option<Slot>,
    /// 0 = top of its stack.
    pub stack_index: u32,
    /// Remembered geometry for Floating restore.
    pub floating_geom: Rect,
    pub surface: ToplevelSurface,
    /// Stable key for surface→window lookup.
    pub surface_id: ObjectId,
    /// Urgent state (§22.6): window requested focus but was blocked by focus-steal prevention.
    pub is_urgent: bool,
    /// Wayland client PID (from `get_credentials`). Used for window swallowing (§22.4).
    pub pid: Option<u32>,
    /// If this window is "swallowing" another, this holds the hidden window's ID (§22.4).
    pub swallowed_id: Option<WindowId>,
    /// True when this window has been hidden by a child that swallowed it (§22.4).
    pub is_swallowed: bool,
    /// Window opacity override from rules (0.0–1.0; 1.0 = opaque).
    pub opacity: f32,
    /// Pinned to all workspaces (visible everywhere). Set via rule `pin`/`sticky`.
    pub sticky: bool,
    /// Skip all animations for this window. Set via rule `no_animations`.
    pub no_animations: bool,
    /// Render above TotalFullscreen on the same workspace. Set via rule; defaults true.
    pub above_total_fullscreen: bool,
    /// Custom border color override (hex string) from a rule.
    pub border_color: Option<String>,
    /// Path to per-window GLSL shader from a rule.
    pub shader: Option<String>,
    /// Allow this window to steal focus even with focus_steal_prevention on (§22.5).
    pub steal_focus: bool,
}
