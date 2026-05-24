use std::collections::{HashMap, VecDeque};

use super::window::{Slot, WindowId, WindowState};

/// A single (col, row) cell on the grid (§5).
pub struct Workspace {
    pub col: u8,
    pub row: u8,
    pub name: Option<String>,
    /// Six tiled slot stacks; index 0 = top (visible).
    pub slots: HashMap<Slot, Vec<WindowId>>,
    /// Floating windows in Z order (bottom to top within floating layer).
    pub floating: Vec<WindowId>,
    /// Fullscreen promotion stack; index 0 = top.
    pub fullscreen_stack: Vec<WindowId>,
    /// TotalFullscreen promotion stack; index 0 = top.
    pub total_fullscreen_stack: Vec<WindowId>,
    /// Per-workspace focus history (most-recent first).
    pub focus_history: VecDeque<WindowId>,
}

impl Workspace {
    pub fn new(col: u8, row: u8) -> Self {
        Self {
            col,
            row,
            name: None,
            slots: HashMap::new(),
            floating: Vec::new(),
            fullscreen_stack: Vec::new(),
            total_fullscreen_stack: Vec::new(),
            focus_history: VecDeque::new(),
        }
    }

    // ── Counts ───────────────────────────────────────────────────────────────

    pub fn tiled_count(&self) -> usize {
        self.slots.values().map(|s| s.len()).sum()
    }

    pub fn window_count(&self) -> usize {
        self.tiled_count()
            + self.floating.len()
            + self.fullscreen_stack.len()
            + self.total_fullscreen_stack.len()
    }

    pub fn is_empty(&self) -> bool {
        self.window_count() == 0
    }

    pub fn has_total_fullscreen(&self) -> bool {
        !self.total_fullscreen_stack.is_empty()
    }

    /// True when there is exactly one window (tiled) and no other windows —
    /// used to decide whether smart gaps should collapse.
    pub fn should_use_smart_gaps(&self) -> bool {
        self.tiled_count() == 1
            && self.floating.is_empty()
            && self.fullscreen_stack.is_empty()
            && self.total_fullscreen_stack.is_empty()
    }

    /// True when only one Fullscreen-promoted window exists and nothing else.
    pub fn has_only_fullscreen(&self) -> bool {
        self.fullscreen_stack.len() == 1
            && self.total_fullscreen_stack.is_empty()
            && self.slots.values().all(|s| s.is_empty())
            && self.floating.is_empty()
    }

    // ── Slot queries ─────────────────────────────────────────────────────────

    pub fn slot_top(&self, slot: Slot) -> Option<WindowId> {
        self.slots.get(&slot).and_then(|s| s.first().copied())
    }

    pub fn slot_stack(&self, slot: Slot) -> &[WindowId] {
        self.slots.get(&slot).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn slot_stack_mut(&mut self, slot: Slot) -> &mut Vec<WindowId> {
        self.slots.entry(slot).or_default()
    }

    /// Occupied slots (those with at least one window).
    pub fn occupied_slots(&self) -> impl Iterator<Item = Slot> + '_ {
        Slot::ALL.iter().copied().filter(|&s| {
            self.slots.get(&s).map(|v| !v.is_empty()).unwrap_or(false)
        })
    }

    // ── Mutations ────────────────────────────────────────────────────────────

    pub fn push_to_slot_top(&mut self, slot: Slot, id: WindowId) {
        self.slot_stack_mut(slot).insert(0, id);
        self.reindex_slot(slot);
    }

    pub fn add_floating(&mut self, id: WindowId) {
        self.floating.push(id);
    }

    pub fn push_to_promotion_top(&mut self, state: WindowState, id: WindowId) {
        match state {
            WindowState::Fullscreen => self.fullscreen_stack.insert(0, id),
            WindowState::TotalFullscreen => self.total_fullscreen_stack.insert(0, id),
            _ => {}
        }
    }

    /// Remove a window from every list in this workspace.
    pub fn remove_window(&mut self, id: WindowId) {
        for stack in self.slots.values_mut() {
            stack.retain(|&w| w != id);
        }
        self.floating.retain(|&w| w != id);
        self.fullscreen_stack.retain(|&w| w != id);
        self.total_fullscreen_stack.retain(|&w| w != id);
        self.focus_history.retain(|&w| w != id);
        // Reindex all slots after removal
        for slot in Slot::ALL {
            self.reindex_slot(slot);
        }
    }

    pub fn record_focus(&mut self, id: WindowId) {
        self.focus_history.retain(|&w| w != id);
        self.focus_history.push_front(id);
    }

    /// Returns updated (id, new_stack_index) pairs for the slot.
    pub fn reindex_slot(&mut self, slot: Slot) -> Vec<(WindowId, u32)> {
        self.slots
            .get(&slot)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, i as u32))
            .collect()
    }

    // ── Z-order ──────────────────────────────────────────────────────────────

    /// All visible windows in bottom→top Z order for rendering (§6.4).
    /// Layer-shell surfaces are handled separately (Phase 6).
    pub fn windows_in_z_order(&self) -> Vec<WindowId> {
        let mut result = Vec::new();
        // 1. Tiled tops
        for slot in Slot::SCAN_ORDER {
            if let Some(top) = self.slot_top(slot) {
                result.push(top);
            }
        }
        // 2. Floating below TotalFullscreen (simplified for Phase 1;
        //    above_total_fullscreen per-window flag is Phase 4)
        // 3. Fullscreen stack top
        if let Some(&top) = self.fullscreen_stack.first() {
            result.push(top);
        }
        // 4. TotalFullscreen stack top
        if let Some(&top) = self.total_fullscreen_stack.first() {
            result.push(top);
        }
        // 5. Floating (above TotalFullscreen by default §6.4)
        for &id in &self.floating {
            result.push(id);
        }
        result
    }
}
