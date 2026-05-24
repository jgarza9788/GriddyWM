//! Animation curves and workspace slide state machine (§7.1, §11.8).

use std::time::{Duration, Instant};

use crate::grid::window::Rect;

// ─── Easing functions ─────────────────────────────────────────────────────────

pub fn ease_out_cubic(t: f32) -> f32 {
    let t1 = t - 1.0;
    t1 * t1 * t1 + 1.0
}

pub fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let t1 = 2.0 * t - 2.0;
        0.5 * t1 * t1 * t1 + 1.0
    }
}

// ─── Workspace slide direction ────────────────────────────────────────────────

/// Normalized direction vector from `old` workspace to `new` workspace.
/// For a 1-cell orthogonal move this is (±1, 0) or (0, ±1).
/// For diagonal moves the vector is normalized (length 1).
pub fn slide_direction(old: (u8, u8), new: (u8, u8)) -> (f32, f32) {
    let dx = new.0 as f32 - old.0 as f32;
    let dy = new.1 as f32 - old.1 as f32;
    let mag = (dx * dx + dy * dy).sqrt();
    if mag < 1e-4 { (0.0, 0.0) } else { (dx / mag, dy / mag) }
}

// ─── Workspace slide ──────────────────────────────────────────────────────────

/// In-progress workspace-slide animation.
#[derive(Debug, Clone)]
pub struct WorkspaceSlide {
    /// Workspace that is sliding away (the one we left).
    pub old_workspace: (u8, u8),
    /// Normalized direction vector from old→new workspace.
    /// e.g. `(1, 0)` = moved right, `(0, -1)` = moved up.
    pub dir: (f32, f32),
    pub started_at: Instant,
    pub duration: Duration,
}

impl WorkspaceSlide {
    pub fn new(old: (u8, u8), dir: (f32, f32), duration_ms: u64) -> Self {
        Self {
            old_workspace: old,
            dir,
            started_at: Instant::now(),
            duration: Duration::from_millis(duration_ms),
        }
    }

    /// Eased progress [0.0 = just started, 1.0 = fully complete].
    pub fn progress(&self) -> f32 {
        let elapsed = self.started_at.elapsed().as_secs_f32();
        let total = self.duration.as_secs_f32().max(f32::EPSILON);
        ease_out_cubic((elapsed / total).min(1.0))
    }

    pub fn is_done(&self) -> bool {
        self.started_at.elapsed() >= self.duration
    }

    /// Pixel offset to apply to the **new** workspace's windows.
    /// Starts offset from the direction the slide came from, animates to (0, 0).
    pub fn new_ws_offset(&self, output_w: i32, output_h: i32) -> (i32, i32) {
        let p = 1.0 - self.progress();
        (
            (self.dir.0 * output_w as f32 * p) as i32,
            (self.dir.1 * output_h as f32 * p) as i32,
        )
    }

    /// Pixel offset to apply to the **old** workspace's windows.
    /// Starts at (0, 0), slides off-screen in the direction of travel.
    pub fn old_ws_offset(&self, output_w: i32, output_h: i32) -> (i32, i32) {
        let p = self.progress();
        (
            (-self.dir.0 * output_w as f32 * p) as i32,
            (-self.dir.1 * output_h as f32 * p) as i32,
        )
    }
}

// ─── Window move animation ────────────────────────────────────────────────────

/// In-progress slot/position transition for a single window.
#[derive(Debug, Clone)]
pub struct MoveAnim {
    pub old_rect: Rect,
    pub started_at: Instant,
    pub duration: Duration,
}

impl MoveAnim {
    pub fn new(old_rect: Rect, duration_ms: u64) -> Self {
        Self {
            old_rect,
            started_at: Instant::now(),
            duration: Duration::from_millis(duration_ms),
        }
    }

    pub fn is_done(&self) -> bool {
        self.started_at.elapsed() >= self.duration
    }

    /// Returns the interpolated rect given the window's current (destination) rect.
    pub fn lerp_rect(&self, new_rect: Rect) -> Rect {
        let t = self.started_at.elapsed().as_secs_f32()
            / self.duration.as_secs_f32().max(f32::EPSILON);
        let p = ease_out_cubic(t.min(1.0));
        let lerp = |a: i32, b: i32| -> i32 { (a as f32 + (b - a) as f32 * p) as i32 };
        Rect::new(
            lerp(self.old_rect.x, new_rect.x),
            lerp(self.old_rect.y, new_rect.y),
            lerp(self.old_rect.w, new_rect.w),
            lerp(self.old_rect.h, new_rect.h),
        )
    }
}

// ─── Window open/close animation ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowAnimKind { Open, Close }

#[derive(Debug, Clone)]
pub struct WindowAnim {
    pub kind: WindowAnimKind,
    pub started_at: Instant,
    pub duration: Duration,
}

impl WindowAnim {
    pub fn open(duration_ms: u64) -> Self {
        Self { kind: WindowAnimKind::Open, started_at: Instant::now(), duration: Duration::from_millis(duration_ms) }
    }

    /// Returns eased progress [0.0..1.0], or None when complete.
    pub fn progress(&self) -> Option<f32> {
        let t = self.started_at.elapsed().as_secs_f32() / self.duration.as_secs_f32().max(f32::EPSILON);
        if t >= 1.0 { None } else { Some(ease_out_cubic(t)) }
    }
}
