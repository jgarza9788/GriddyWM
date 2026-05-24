//! Focus-on-close (§6.10) and spatial focus navigation (§6.11).

use super::window::{Slot, WindowId};
use super::workspace::Workspace;
use crate::config::types::FocusOnClosePolicy;

// ─── Focus-on-close (§6.10) ──────────────────────────────────────────────────

/// Determine which window to focus after `closed_id` is removed.
/// Returns None if no window remains to focus on this workspace.
pub fn focus_after_close(
    ws: &Workspace,
    closed_id: WindowId,
    closed_slot: Option<Slot>,
    policy: &FocusOnClosePolicy,
) -> Option<WindowId> {
    match policy {
        FocusOnClosePolicy::LastFocused => last_focused(ws, closed_id),

        FocusOnClosePolicy::StackNext => {
            if let Some(slot) = closed_slot {
                ws.slot_stack(slot)
                    .iter()
                    .find(|&&id| id != closed_id)
                    .copied()
                    .or_else(|| last_focused(ws, closed_id))
            } else {
                last_focused(ws, closed_id)
            }
        }

        FocusOnClosePolicy::SlotNeighbor => {
            if let Some(slot) = closed_slot {
                nearest_occupied(ws, slot)
                    .and_then(|s| ws.slot_top(s))
                    .or_else(|| last_focused(ws, closed_id))
            } else {
                last_focused(ws, closed_id)
            }
        }

        FocusOnClosePolicy::None => None,
    }
}

fn last_focused(ws: &Workspace, closed_id: WindowId) -> Option<WindowId> {
    ws.focus_history
        .iter()
        .find(|&&id| id != closed_id)
        .copied()
}

/// Find the occupied slot geometrically nearest to `from_slot`.
fn nearest_occupied(ws: &Workspace, from_slot: Slot) -> Option<Slot> {
    let from = from_slot.center_norm();
    ws.occupied_slots()
        .filter(|&s| s != from_slot)
        .min_by(|&a, &b| {
            let ac = a.center_norm();
            let bc = b.center_norm();
            let da = (ac.0 - from.0).powi(2) + (ac.1 - from.1).powi(2);
            let db = (bc.0 - from.0).powi(2) + (bc.1 - from.1).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
}

// ─── Spatial focus navigation (§6.11) ────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Find the best window to focus in `direction` from `focused_slot`.
///
/// Returns the window ID if one is found within the workspace.
/// Returns None if focus should cross into the adjacent workspace (caller decides).
pub fn spatial_focus(
    ws: &Workspace,
    focused_slot: Slot,
    direction: FocusDirection,
) -> Option<WindowId> {
    let (fx, fy) = focused_slot.center_norm();

    // Candidate slots whose center lies in the requested direction.
    let candidates: Vec<Slot> = ws
        .occupied_slots()
        .filter(|&s| {
            if s == focused_slot {
                return false;
            }
            let (cx, cy) = s.center_norm();
            match direction {
                FocusDirection::Left => cx < fx,
                FocusDirection::Right => cx > fx,
                FocusDirection::Up => cy < fy,
                FocusDirection::Down => cy > fy,
            }
        })
        .collect();

    if candidates.is_empty() {
        return None; // caller should switch workspace
    }

    // Pick the nearest by Euclidean distance.
    candidates
        .into_iter()
        .min_by(|&a, &b| {
            let ac = a.center_norm();
            let bc = b.center_norm();
            let da = (ac.0 - fx).powi(2) + (ac.1 - fy).powi(2);
            let db = (bc.0 - fx).powi(2) + (bc.1 - fy).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|s| ws.slot_top(s))
}
