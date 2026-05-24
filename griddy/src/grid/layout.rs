use super::window::{Rect, Slot};

/// Default gap sizes used until theme.toml parsing is implemented (Phase 2).
pub const DEFAULT_INNER_PX: i32 = 8;
pub const DEFAULT_OUTER_PX: i32 = 12;

/// Compute the pixel geometry of `slot` within a single output.
///
/// When `smart_gaps` is true (workspace has exactly one tiled window),
/// all gaps collapse to zero so the window fills the screen.
pub fn slot_rect(
    slot: Slot,
    output_w: i32,
    output_h: i32,
    inner_px: i32,
    outer_px: i32,
    smart_gaps: bool,
) -> Rect {
    let (ol, or_, ot, ob, inner) = if smart_gaps {
        (0, 0, 0, 0, 0)
    } else {
        (outer_px, outer_px, outer_px, outer_px, inner_px)
    };

    let usable_w = (output_w - ol - or_).max(1);
    let usable_h = (output_h - ot - ob).max(1);
    let half_inner = inner / 2;

    let left_w = (usable_w / 2 - half_inner).max(1);
    let right_w = (usable_w - left_w - inner).max(1);
    let top_h = (usable_h / 2 - half_inner).max(1);
    let bot_h = (usable_h - top_h - inner).max(1);

    let x_l = ol;
    let x_r = ol + left_w + inner;
    let y_t = ot;
    let y_b = ot + top_h + inner;

    match slot {
        Slot::HalfLeft => Rect::new(x_l, y_t, left_w, usable_h),
        Slot::HalfRight => Rect::new(x_r, y_t, right_w, usable_h),
        Slot::QuarterTL => Rect::new(x_l, y_t, left_w, top_h),
        Slot::QuarterTR => Rect::new(x_r, y_t, right_w, top_h),
        Slot::QuarterBL => Rect::new(x_l, y_b, left_w, bot_h),
        Slot::QuarterBR => Rect::new(x_r, y_b, right_w, bot_h),
    }
}

/// Fullscreen state: usable area (respects outer gaps, no inner gaps).
pub fn fullscreen_rect(output_w: i32, output_h: i32, outer_px: i32) -> Rect {
    Rect::new(
        outer_px,
        outer_px,
        (output_w - 2 * outer_px).max(1),
        (output_h - 2 * outer_px).max(1),
    )
}

/// TotalFullscreen: entire output, no gaps (§6.3).
pub fn total_fullscreen_rect(output_w: i32, output_h: i32) -> Rect {
    Rect::new(0, 0, output_w, output_h)
}

/// Compute the screen-space rect for a workspace thumbnail in overview mode (§7.2).
///
/// `gap_px` is the gap between thumbnails; `margin_px` is the outer margin.
pub fn overview_thumbnail_rect(
    col: u8, row: u8,
    cols: u8, rows: u8,
    screen_w: i32, screen_h: i32,
    gap_px: i32, margin_px: i32,
) -> Rect {
    let cols = cols.max(1) as i32;
    let rows = rows.max(1) as i32;
    let avail_w = (screen_w - 2 * margin_px - (cols - 1) * gap_px).max(cols);
    let avail_h = (screen_h - 2 * margin_px - (rows - 1) * gap_px).max(rows);
    let thumb_w = avail_w / cols;
    let thumb_h = avail_h / rows;
    let tx = margin_px + col as i32 * (thumb_w + gap_px);
    let ty = margin_px + row as i32 * (thumb_h + gap_px);
    Rect::new(tx, ty, thumb_w.max(1), thumb_h.max(1))
}

/// Compute the screen-space rect for a single minimap cell (§7.4).
///
/// `anchor_x/y` is the top-left of the entire minimap widget.
pub fn minimap_cell_rect(
    col: u8, row: u8,
    cell_px: i32, gap_px: i32,
    anchor_x: i32, anchor_y: i32,
) -> Rect {
    let tx = anchor_x + col as i32 * (cell_px + gap_px);
    let ty = anchor_y + row as i32 * (cell_px + gap_px);
    Rect::new(tx, ty, cell_px.max(1), cell_px.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::window::Slot;

    const W: i32 = 1920;
    const H: i32 = 1080;
    const INNER: i32 = 8;
    const OUTER: i32 = 12;

    #[test]
    fn smart_gaps_half_left_fills_screen() {
        let r = slot_rect(Slot::HalfLeft, W, H, INNER, OUTER, true);
        assert_eq!(r.x, 0);
        assert_eq!(r.y, 0);
        assert_eq!(r.w, W / 2);
        assert_eq!(r.h, H);
    }

    #[test]
    fn half_left_plus_half_right_covers_usable_width() {
        let left  = slot_rect(Slot::HalfLeft,  W, H, INNER, OUTER, false);
        let right = slot_rect(Slot::HalfRight, W, H, INNER, OUTER, false);
        let usable_w = W - 2 * OUTER;
        assert_eq!(left.w + right.w + INNER, usable_w);
    }

    #[test]
    fn quarter_tl_tr_share_top_row() {
        let tl = slot_rect(Slot::QuarterTL, W, H, INNER, OUTER, false);
        let tr = slot_rect(Slot::QuarterTR, W, H, INNER, OUTER, false);
        assert_eq!(tl.y, tr.y);
        assert_eq!(tl.h, tr.h);
    }

    #[test]
    fn quarter_tl_bl_share_left_col() {
        let tl = slot_rect(Slot::QuarterTL, W, H, INNER, OUTER, false);
        let bl = slot_rect(Slot::QuarterBL, W, H, INNER, OUTER, false);
        assert_eq!(tl.x, bl.x);
        assert_eq!(tl.w, bl.w);
    }

    #[test]
    fn total_fullscreen_is_whole_output() {
        let r = total_fullscreen_rect(W, H);
        assert_eq!((r.x, r.y, r.w, r.h), (0, 0, W, H));
    }

    #[test]
    fn fullscreen_respects_outer_gap() {
        let r = fullscreen_rect(W, H, OUTER);
        assert_eq!(r.x, OUTER);
        assert_eq!(r.y, OUTER);
        assert_eq!(r.w, W - 2 * OUTER);
        assert_eq!(r.h, H - 2 * OUTER);
    }

    #[test]
    fn overview_thumbnail_grid_2x2_origin() {
        let r = overview_thumbnail_rect(0, 0, 2, 2, W, H, 16, 32);
        assert_eq!(r.x, 32);
        assert_eq!(r.y, 32);
    }

    #[test]
    fn overview_thumbnail_col1_offset() {
        let r0 = overview_thumbnail_rect(0, 0, 2, 2, W, H, 16, 32);
        let r1 = overview_thumbnail_rect(1, 0, 2, 2, W, H, 16, 32);
        assert_eq!(r1.x, r0.x + r0.w + 16);
    }

    #[test]
    fn minimap_cell_position() {
        let r = minimap_cell_rect(1, 2, 14, 3, 100, 200);
        assert_eq!(r.x, 100 + 1 * (14 + 3));
        assert_eq!(r.y, 200 + 2 * (14 + 3));
        assert_eq!(r.w, 14);
        assert_eq!(r.h, 14);
    }
}
