//! New-window placement policy (§6.9).

use super::conflict;
use super::window::{Slot, WindowId, WindowState};
use super::workspace::Workspace;

/// Initial state/slot assignment for a new window.
pub struct InitialPlacement {
    pub state: WindowState,
    /// Some only when state == Tiled.
    pub slot: Option<Slot>,
}

/// A window that must be demoted (e.g. a Fullscreen window adapting down
/// when a second window arrives on the same workspace).
pub struct DemoteAction {
    pub window_id: WindowId,
    pub to_slot: Slot,
    pub new_state: WindowState,
}

/// Determine the initial placement for a newly mapped window plus any
/// demotion actions that must be applied to existing windows.
///
/// This is the "before adaptation" step — the result is fed into
/// `conflict::resolve` if a slot conflict exists.
pub fn place_new_window(ws: &Workspace) -> (InitialPlacement, Vec<DemoteAction>) {
    // Rule 1: truly empty workspace → first window gets Fullscreen (§6.9).
    if ws.is_empty() {
        return (
            InitialPlacement {
                state: WindowState::Fullscreen,
                slot: None,
            },
            vec![],
        );
    }

    // Rule 2: exactly one Fullscreen-promoted window, nothing else →
    // demote it to HalfLeft and put the new window in HalfRight.
    // (This produces the natural 50/50 split described in §6.9.)
    if ws.has_only_fullscreen() {
        let demoted_id = ws.fullscreen_stack[0];
        return (
            InitialPlacement {
                state: WindowState::Tiled,
                slot: Some(Slot::HalfRight),
            },
            vec![DemoteAction {
                window_id: demoted_id,
                to_slot: Slot::HalfLeft,
                new_state: WindowState::Tiled,
            }],
        );
    }

    // Rule 3: next-empty-slot scan (§6.9 scan order).
    // Skip slots that geometrically conflict with any occupied slot.
    let occupied_set: std::collections::HashSet<Slot> = ws.occupied_slots().collect();

    for &candidate in &Slot::SCAN_ORDER {
        if ws.slot_top(candidate).is_some() {
            continue; // slot stack already has windows
        }
        let blocked = occupied_set
            .iter()
            .any(|&occ| occ != candidate && conflict::slots_conflict(candidate, occ));
        if !blocked {
            return (
                InitialPlacement {
                    state: WindowState::Tiled,
                    slot: Some(candidate),
                },
                vec![],
            );
        }
    }

    // Rule 4: all non-conflicting slots are occupied — stack onto the
    // first occupied slot (default: HalfLeft or whichever is first).
    let target = ws.occupied_slots().next().unwrap_or(Slot::HalfLeft);

    (
        InitialPlacement {
            state: WindowState::Tiled,
            slot: Some(target),
        },
        vec![],
    )
}
