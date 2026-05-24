//! Slot adaptation and conflict resolution (§6.5).

use super::window::{Slot, WindowId, WindowState};
use super::workspace::Workspace;
use crate::config::types::ConflictPolicy;

// ─── Coexistence matrix ───────────────────────────────────────────────────────

/// Returns true if slots `a` and `b` geometrically conflict (§6.2).
/// Two slots conflict when they overlap in screen space.
pub fn slots_conflict(a: Slot, b: Slot) -> bool {
    if a == b {
        return true;
    }
    use Slot::*;
    matches!(
        (a, b),
        (HalfLeft, QuarterTL)
            | (QuarterTL, HalfLeft)
            | (HalfLeft, QuarterBL)
            | (QuarterBL, HalfLeft)
            | (HalfRight, QuarterTR)
            | (QuarterTR, HalfRight)
            | (HalfRight, QuarterBR)
            | (QuarterBR, HalfRight)
    )
}

/// Occupied slots that geometrically conflict with `target`.
pub fn conflicting_occupants(ws: &Workspace, target: Slot) -> Vec<Slot> {
    ws.occupied_slots()
        .filter(|&occ| slots_conflict(occ, target))
        .collect()
}

// ─── Slot adaptation (§6.5.1) ────────────────────────────────────────────────

/// Half → complementary Quarter adaptation.
/// Returns the Quarter to adapt into, or None if adaptation does not apply.
pub fn adapt_half_to_quarter(ws: &Workspace, incoming: Slot) -> Option<Slot> {
    use Slot::*;
    match incoming {
        HalfLeft => {
            let tl = ws.slot_top(QuarterTL).is_some();
            let bl = ws.slot_top(QuarterBL).is_some();
            match (tl, bl) {
                (true, false) => Some(QuarterBL),
                (false, true) => Some(QuarterTL),
                _ => None,
            }
        }
        HalfRight => {
            let tr = ws.slot_top(QuarterTR).is_some();
            let br = ws.slot_top(QuarterBR).is_some();
            match (tr, br) {
                (true, false) => Some(QuarterBR),
                (false, true) => Some(QuarterTR),
                _ => None,
            }
        }
        _ => None, // Quarters don't adapt into Halves
    }
}

fn left_side_free(ws: &Workspace) -> bool {
    ws.slot_top(Slot::HalfLeft).is_none()
        && ws.slot_top(Slot::QuarterTL).is_none()
        && ws.slot_top(Slot::QuarterBL).is_none()
}

fn right_side_free(ws: &Workspace) -> bool {
    ws.slot_top(Slot::HalfRight).is_none()
        && ws.slot_top(Slot::QuarterTR).is_none()
        && ws.slot_top(Slot::QuarterBR).is_none()
}

/// Fullscreen adaptation cascade (§6.5.1 steps 1–5).
///
/// Returns `Some(Slot)` → adapt to Tiled/slot.
/// Returns `None` → place in Fullscreen promotion stack.
pub fn adapt_fullscreen(ws: &Workspace, drag_target: Option<Slot>) -> Option<Slot> {
    use Slot::*;

    // Step 1: no tiled occupants → Fullscreen promotion
    if ws.tiled_count() == 0 {
        return None;
    }

    // Step 2: left side fully free, right has at least one → HalfLeft
    if left_side_free(ws) && !right_side_free(ws) {
        return Some(HalfLeft);
    }

    // Step 3: right side fully free, left has at least one → HalfRight
    if right_side_free(ws) && !left_side_free(ws) {
        return Some(HalfRight);
    }

    // Step 4: first free Quarter in reading order
    for &q in &[QuarterTL, QuarterTR, QuarterBL, QuarterBR] {
        if ws.slot_top(q).is_none() && conflicting_occupants(ws, q).is_empty() {
            return Some(q);
        }
    }

    // Step 5: all quarters blocked → drag target or QuarterTL default
    Some(drag_target.unwrap_or(QuarterTL))
}

// ─── Full two-pass resolution ─────────────────────────────────────────────────

/// What to do with a window displaced by a Swap or inter-slot conflict.
pub struct DisplacedWindow {
    pub id: WindowId,
    pub from_slot: Slot,
}

pub enum Placement {
    /// Place in a tiled slot (possibly after adaptation).
    Tiled {
        slot: Slot,
        /// True if the slot was adapted from the original request.
        adapted: bool,
    },
    /// Place in the Fullscreen promotion stack.
    FullscreenPromotion,
    /// Place as Floating (kick-to-floating policy or Floating state).
    Floating,
}

pub struct ResolveResult {
    pub placement: Placement,
    /// Windows to move to Floating because they were kicked out.
    pub displaced: Vec<DisplacedWindow>,
}

/// Full two-pass: adaptation → conflict resolution (§6.5).
#[allow(clippy::too_many_arguments)]
pub fn resolve(
    ws: &Workspace,
    incoming_state: WindowState,
    requested_slot: Option<Slot>,
    drag_target: Option<Slot>,
    policy: &ConflictPolicy,
    slot_adaptation: bool,
    fullscreen_adaptation: bool,
) -> ResolveResult {
    match incoming_state {
        WindowState::TotalFullscreen => ResolveResult {
            placement: Placement::FullscreenPromotion,
            displaced: vec![],
        },

        WindowState::Fullscreen => {
            if fullscreen_adaptation {
                match adapt_fullscreen(ws, drag_target) {
                    None => ResolveResult {
                        placement: Placement::FullscreenPromotion,
                        displaced: vec![],
                    },
                    Some(slot) => resolve_slot(ws, slot, true, policy),
                }
            } else {
                ResolveResult {
                    placement: Placement::FullscreenPromotion,
                    displaced: vec![],
                }
            }
        }

        WindowState::Floating => ResolveResult {
            placement: Placement::Floating,
            displaced: vec![],
        },

        WindowState::Tiled => {
            let Some(slot) = requested_slot else {
                return ResolveResult {
                    placement: Placement::Floating,
                    displaced: vec![],
                };
            };

            // Pass 1: Half → Quarter adaptation
            if slot_adaptation
                && slot.is_half()
                && let Some(adapted) = adapt_half_to_quarter(ws, slot)
            {
                return ResolveResult {
                    placement: Placement::Tiled {
                        slot: adapted,
                        adapted: true,
                    },
                    displaced: vec![],
                };
            }

            // Pass 2: conflict resolution
            resolve_slot(ws, slot, false, policy)
        }
    }
}

fn resolve_slot(
    ws: &Workspace,
    slot: Slot,
    adapted: bool,
    policy: &ConflictPolicy,
) -> ResolveResult {
    let conflicts = conflicting_occupants(ws, slot);

    if conflicts.is_empty() {
        return ResolveResult {
            placement: Placement::Tiled { slot, adapted },
            displaced: vec![],
        };
    }

    match policy {
        ConflictPolicy::Stack => ResolveResult {
            // Incoming joins the top of the existing stack — no displacement needed.
            placement: Placement::Tiled { slot, adapted },
            displaced: vec![],
        },
        ConflictPolicy::Swap => {
            let displaced = conflicts
                .into_iter()
                .filter_map(|s| {
                    ws.slot_top(s)
                        .map(|id| DisplacedWindow { id, from_slot: s })
                })
                .collect();
            ResolveResult {
                placement: Placement::Tiled { slot, adapted },
                displaced,
            }
        }
        ConflictPolicy::KickToFloating => ResolveResult {
            placement: Placement::Floating,
            displaced: vec![],
        },
    }
}
