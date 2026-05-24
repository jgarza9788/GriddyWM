//! Action enum and dispatcher — maps keybind actions to compositor state mutations.

use smithay::utils::{Serial, SERIAL_COUNTER};

use crate::animate::{self, MoveAnim, WorkspaceSlide};
use crate::grid::{window::Slot, FocusDirection, MoveDirectionResult, PlacementHints};
use crate::ipc::events::Event;
use crate::state::GlobalState;

// ─── Action ───────────────────────────────────────────────────────────────────

/// All compositor actions that can be bound to a key.
#[derive(Debug, Clone)]
pub enum Action {
    Exec(String),
    CloseWindow,
    Quit,
    ReloadConfig,
    ReloadTheme,
    CenterFloating,

    // UI stubs (parsed, logged, no-op until implemented)
    OsdShow(String),
    CycleShader,
    CheatsheetToggle,
    DebugOverlayToggle,

    // Workspace navigation
    WorkspaceLeft,
    WorkspaceRight,
    WorkspaceUp,
    WorkspaceDown,
    WorkspaceNW,
    WorkspaceNE,
    WorkspaceSW,
    WorkspaceSE,
    WorkspaceBack,
    WorkspaceForward,
    WorkspaceIndex(u8),
    WorkspaceTo(u8, u8),

    // Focus navigation
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,

    // Window state
    StateFullscreenToggle,
    StateTotalFullscreenToggle,
    StateFloatingToggle,

    // Slot assignment
    SlotHalfLeft,
    SlotHalfRight,
    SlotQuarterTL,
    SlotQuarterTR,
    SlotQuarterBL,
    SlotQuarterBR,

    // Stack operations
    StackNext,
    StackPrev,
    StackPromote,
    StackCollapse,
    StackMoveUp,
    StackMoveDown,
    /// Jump directly to stack index n (0 = top).
    StackFlip(u32),

    // Window movement
    MoveWindowDirection(FocusDirection),
    MoveWindowToIndex(u8),
    MoveWindowTo(u8, u8),
    MoveWindowToWorkspaceAndFollow(u8, u8),

    // Spawn with hint
    SpawnFloating(String),
    SpawnInSlot(Slot, String),
    SpawnOnWorkspace(u8, u8, String),
    SpawnStacked(String),

    // State exits
    TotalFullscreenExit,

    // Workspace meta
    WorkspaceSync,
    WorkspaceRename(u8, u8, String),

    // UI overlays
    MinimapToggle,

    // Resize (floating windows)
    ResizeActive(i32, i32),

    // Submaps (§8.7)
    Submap(String),
    SubmapReset,
    /// Dispatch the inner action, then exit the active submap.
    SubmapActionThenReset(Box<Action>),

    // Overview mode (§7.2)
    OverviewToggle,
    OverviewFocusLeft,
    OverviewFocusRight,
    OverviewFocusUp,
    OverviewFocusDown,
    OverviewActivate,
    /// Tab — cycle focus to next window in the focused thumbnail.
    OverviewWindowNext,
    /// Shift+Tab — cycle focus to previous window in the focused thumbnail.
    OverviewWindowPrev,
    /// Space — pick up focused window so arrow keys can drag it between thumbnails.
    OverviewGrabWindow,
    /// Drop a grabbed window (same as OverviewActivate when grab is active).
    OverviewDropWindow,
    /// Cancel grab and return window to its origin workspace.
    OverviewCancelGrab,

    // Screenshot
    Screenshot(String),

    // Workspace templates (§8.10)
    /// Apply the template for the workspace at `[col, row]`, even if already applied.
    ApplyTemplate(u8, u8),

    // Special workspaces / scratchpad (§22.3)
    /// Show/hide a named special workspace (toggle).
    ToggleSpecial(String),
    /// Move the focused window into a named special workspace.
    MoveToSpecial(String),

    // Peek modes (release-bind actions §9) ─────────────────────────────────
    /// Hold to show overview; fires on key press (enter) and on key release (exit).
    OverviewPeek,
    /// Hold to fan out stack preview; fires on key press (enter) and release (exit).
    StackPeek,

    // Shader control (§11)
    /// Set per-window GLSL shader: `set-window-shader <id> <path|clear>`.
    SetWindowShader(u64, String),
    /// Set / clear the global screen post-process shader.
    ScreenShader(String),
}

// ─── Parser ───────────────────────────────────────────────────────────────────

/// Convert an action string + args into an `Action`, or `None` if unrecognized.
pub fn parse_action(action: &str, args: &[String]) -> Option<Action> {
    // Inline "exec <cmd>" form: action = "exec kitty"
    if let Some(cmd) = action.strip_prefix("exec ") {
        return Some(Action::Exec(cmd.trim().to_owned()));
    }
    match action {
        "exec" => {
            let cmd = args.join(" ");
            (!cmd.is_empty()).then_some(Action::Exec(cmd))
        }
        "close-window"                   => Some(Action::CloseWindow),
        "quit"                           => Some(Action::Quit),
        "reload-config"                  => Some(Action::ReloadConfig),
        "reload-theme"                   => Some(Action::ReloadTheme),
        "osd-show" => {
            let text = args.first().cloned().unwrap_or_default();
            Some(Action::OsdShow(text))
        }
        "cycle-shader"                   => Some(Action::CycleShader),
        "cheatsheet-toggle"              => Some(Action::CheatsheetToggle),
        "debug-overlay-toggle"           => Some(Action::DebugOverlayToggle),
        "center-floating"                => Some(Action::CenterFloating),
        "workspace-left"                 => Some(Action::WorkspaceLeft),
        "workspace-right"                => Some(Action::WorkspaceRight),
        "workspace-up"                   => Some(Action::WorkspaceUp),
        "workspace-down"                 => Some(Action::WorkspaceDown),
        "workspace-nw"                   => Some(Action::WorkspaceNW),
        "workspace-ne"                   => Some(Action::WorkspaceNE),
        "workspace-sw"                   => Some(Action::WorkspaceSW),
        "workspace-se"                   => Some(Action::WorkspaceSE),
        "workspace-back"                 => Some(Action::WorkspaceBack),
        "workspace-forward"              => Some(Action::WorkspaceForward),
        "workspace-index"                => args.first()?.parse().ok().map(Action::WorkspaceIndex),
        "workspace" => {
            // workspace col,row
            let coord = args.first()?;
            let (c, r) = coord.split_once(',')?;
            Some(Action::WorkspaceTo(c.parse().ok()?, r.parse().ok()?))
        }
        "workspace-direction" => {
            match args.first().map(|s| s.as_str()) {
                Some("left")  => Some(Action::WorkspaceLeft),
                Some("right") => Some(Action::WorkspaceRight),
                Some("up")    => Some(Action::WorkspaceUp),
                Some("down")  => Some(Action::WorkspaceDown),
                _ => None,
            }
        }
        "focus-left"                     => Some(Action::FocusLeft),
        "focus-right"                    => Some(Action::FocusRight),
        "focus-up"                       => Some(Action::FocusUp),
        "focus-down"                     => Some(Action::FocusDown),
        "state-fullscreen-toggle"        => Some(Action::StateFullscreenToggle),
        "state-total-fullscreen-toggle"  => Some(Action::StateTotalFullscreenToggle),
        "state-floating-toggle"          => Some(Action::StateFloatingToggle),
        "slot-half-left"                 => Some(Action::SlotHalfLeft),
        "slot-half-right"                => Some(Action::SlotHalfRight),
        "slot-quarter-tl"                => Some(Action::SlotQuarterTL),
        "slot-quarter-tr"                => Some(Action::SlotQuarterTR),
        "slot-quarter-bl"                => Some(Action::SlotQuarterBL),
        "slot-quarter-br"                => Some(Action::SlotQuarterBR),
        "stack-next"                     => Some(Action::StackNext),
        "stack-prev"                     => Some(Action::StackPrev),
        "stack-promote"                  => Some(Action::StackPromote),
        "stack-collapse"                 => Some(Action::StackCollapse),
        "stack-move-up"                  => Some(Action::StackMoveUp),
        "stack-move-down"                => Some(Action::StackMoveDown),
        "total-fullscreen-exit"          => Some(Action::TotalFullscreenExit),
        "workspace-sync-toggle"          => Some(Action::WorkspaceSync),
        "workspace-rename" => {
            let col: u8 = args.first()?.parse().ok()?;
            let row: u8 = args.get(1)?.parse().ok()?;
            let name = args.get(2).cloned().unwrap_or_default();
            Some(Action::WorkspaceRename(col, row, name))
        }
        "minimap-toggle"                 => Some(Action::MinimapToggle),
        "move-window-to-workspace-and-follow" => {
            let coord = args.first()?;
            let (c, r) = coord.split_once(',')?;
            Some(Action::MoveWindowToWorkspaceAndFollow(c.parse().ok()?, r.parse().ok()?))
        }
        "spawn-floating" => {
            let cmd = args.join(" ");
            (!cmd.is_empty()).then_some(Action::SpawnFloating(cmd))
        }
        "spawn-in-slot" => {
            let slot_str = args.first()?;
            let slot = parse_slot(slot_str)?;
            let cmd = args[1..].join(" ");
            (!cmd.is_empty()).then_some(Action::SpawnInSlot(slot, cmd))
        }
        "spawn-on-workspace" => {
            let coord = args.first()?;
            let (c, r) = coord.split_once(',')?;
            let cmd = args[1..].join(" ");
            (!cmd.is_empty()).then_some(Action::SpawnOnWorkspace(c.parse().ok()?, r.parse().ok()?, cmd))
        }
        "spawn-stacked" => {
            let cmd = args.join(" ");
            (!cmd.is_empty()).then_some(Action::SpawnStacked(cmd))
        }
        "move-window-direction"  => parse_direction(args.first()?).map(Action::MoveWindowDirection),
        "move-window-to-index"   => args.first()?.parse().ok().map(Action::MoveWindowToIndex),
        "move-window-to" => {
            let coord = args.first()?;
            let (c, r) = coord.split_once(',')?;
            Some(Action::MoveWindowTo(c.parse().ok()?, r.parse().ok()?))
        }
        "resize-active"          => {
            let dx = args.first().and_then(|s| s.parse().ok()).unwrap_or(0i32);
            let dy = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0i32);
            Some(Action::ResizeActive(dx, dy))
        }
        "submap" => {
            let name = args.first()?.clone();
            if name == "reset" {
                Some(Action::SubmapReset)
            } else {
                Some(Action::Submap(name))
            }
        }
        "submap-reset" => Some(Action::SubmapReset),
        "overview-toggle"       => Some(Action::OverviewToggle),
        "overview-activate"     => Some(Action::OverviewActivate),
        "overview-window-next"  => Some(Action::OverviewWindowNext),
        "overview-window-prev"  => Some(Action::OverviewWindowPrev),
        "overview-grab-window"  => Some(Action::OverviewGrabWindow),
        "overview-drop-window"  => Some(Action::OverviewDropWindow),
        "overview-cancel-grab"  => Some(Action::OverviewCancelGrab),
        "overview-focus-direction" => {
            match args.first().map(|s| s.as_str()) {
                Some("left")  => Some(Action::OverviewFocusLeft),
                Some("right") => Some(Action::OverviewFocusRight),
                Some("up")    => Some(Action::OverviewFocusUp),
                Some("down")  => Some(Action::OverviewFocusDown),
                _ => None,
            }
        }
        "screenshot" => {
            let mode = args.first().cloned().unwrap_or_else(|| "region".into());
            Some(Action::Screenshot(mode))
        }
        "workspace-apply-template" | "apply-template" => {
            if args.is_empty() {
                // Apply to current focused workspace.
                Some(Action::ApplyTemplate(u8::MAX, u8::MAX))
            } else {
                let coord = args.first()?;
                let (c, r) = coord.split_once(',')?;
                Some(Action::ApplyTemplate(c.parse().ok()?, r.parse().ok()?))
            }
        }
        "toggle-special" => {
            let name = args.first().cloned().unwrap_or_else(|| "scratchpad".into());
            Some(Action::ToggleSpecial(name))
        }
        "move-to-special" => {
            let name = args.first().cloned().unwrap_or_else(|| "scratchpad".into());
            Some(Action::MoveToSpecial(name))
        }
        "stack-flip" => {
            let n: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            Some(Action::StackFlip(n))
        }
        // Alias from §9.2 action reference.
        "move-window" => {
            let coord = args.first()?;
            let (c, r) = coord.split_once(',')?;
            Some(Action::MoveWindowTo(c.parse().ok()?, r.parse().ok()?))
        }
        "overview-peek" => Some(Action::OverviewPeek),
        "stack-peek"    => Some(Action::StackPeek),
        "set-window-shader" => {
            let mut it = args.iter();
            let id: u64 = it.next().and_then(|s| s.parse().ok())?;
            let path = it.cloned().collect::<Vec<_>>().join(" ");
            Some(Action::SetWindowShader(id, path))
        }
        "screen-shader" => {
            let path = args.first().cloned().unwrap_or_default();
            Some(Action::ScreenShader(path))
        }
        _ => None,
    }
}

fn parse_direction(s: &str) -> Option<FocusDirection> {
    match s {
        "left"  => Some(FocusDirection::Left),
        "right" => Some(FocusDirection::Right),
        "up"    => Some(FocusDirection::Up),
        "down"  => Some(FocusDirection::Down),
        _       => None,
    }
}

fn parse_slot(s: &str) -> Option<Slot> {
    match s {
        "HalfLeft"   | "half-left"   => Some(Slot::HalfLeft),
        "HalfRight"  | "half-right"  => Some(Slot::HalfRight),
        "QuarterTL"  | "quarter-tl"  => Some(Slot::QuarterTL),
        "QuarterTR"  | "quarter-tr"  => Some(Slot::QuarterTR),
        "QuarterBL"  | "quarter-bl"  => Some(Slot::QuarterBL),
        "QuarterBR"  | "quarter-br"  => Some(Slot::QuarterBR),
        "Fullscreen" | "fullscreen"  => None, // not a tiled slot
        _ => None,
    }
}

// ─── Dispatcher ───────────────────────────────────────────────────────────────

/// Execute an action. Returns `true` if the compositor should exit.
pub fn dispatch(action: Action, state: &mut GlobalState) -> bool {
    match action {
        Action::Exec(cmd)    => { exec_cmd(&cmd); false }
        Action::CloseWindow  => { state.grid.close_focused(); false }
        Action::Quit         => true,

        Action::ReloadConfig => {
            state.reload_config_if_changed(true);
            false
        }

        Action::ReloadTheme => {
            state.reload_theme();
            false
        }

        Action::OsdShow(text) => {
            tracing::info!(text = %text, "osd-show");
            // Use the text as the OSD kind; render as a custom accent bar.
            state.show_osd(text);
            false
        }
        Action::CycleShader => {
            tracing::info!("cycle-shader (not yet implemented)");
            false
        }
        Action::DebugOverlayToggle => {
            state.debug_overlay = !state.debug_overlay;
            tracing::info!(enabled = state.debug_overlay, "debug-overlay toggled");
            false
        }
        Action::CheatsheetToggle => {
            tracing::info!("cheatsheet-toggle (not yet implemented)");
            false
        }

        Action::CenterFloating => {
            // Center the focused floating window on screen.
            if let Some(id) = state.grid.focused_window {
                if let Some(w) = state.grid.windows.get_mut(&id) {
                    if w.current_state == crate::grid::window::WindowState::Floating {
                        let ow = state.grid.output_w;
                        let oh = state.grid.output_h;
                        let fw = w.floating_geom.w;
                        let fh = w.floating_geom.h;
                        w.floating_geom.x = (ow - fw) / 2;
                        w.floating_geom.y = (oh - fh) / 2;
                        let id = id;
                        state.grid.send_configure(id);
                    }
                }
            }
            false
        }

        Action::WorkspaceLeft  => { nav_workspace(state, FocusDirection::Left);  false }
        Action::WorkspaceRight => { nav_workspace(state, FocusDirection::Right); false }
        Action::WorkspaceUp    => { nav_workspace(state, FocusDirection::Up);    false }
        Action::WorkspaceDown  => { nav_workspace(state, FocusDirection::Down);  false }
        Action::WorkspaceNW    => { nav_workspace_diagonal(state, -1, -1); false }
        Action::WorkspaceNE    => { nav_workspace_diagonal(state,  1, -1); false }
        Action::WorkspaceSW    => { nav_workspace_diagonal(state, -1,  1); false }
        Action::WorkspaceSE    => { nav_workspace_diagonal(state,  1,  1); false }
        Action::WorkspaceBack  => {
            let old = state.grid.focused;
            state.grid.workspace_back();
            trigger_slide(state, old);
            sync_focus(state);
            if state.config.input.warp_cursor_on_workspace_change {
                warp_cursor_to_focused(state);
            }
            emit_workspace_changed(state);
            false
        }
        Action::WorkspaceForward => {
            let old = state.grid.focused;
            state.grid.workspace_forward();
            trigger_slide(state, old);
            sync_focus(state);
            if state.config.input.warp_cursor_on_workspace_change {
                warp_cursor_to_focused(state);
            }
            emit_workspace_changed(state);
            false
        }
        Action::WorkspaceTo(col, row) => {
            let old = state.grid.focused;
            state.grid.switch_workspace((col, row));
            apply_workspace_template(state, (col, row), false);
            trigger_slide(state, old);
            sync_focus(state);
            if state.config.input.warp_cursor_on_workspace_change {
                warp_cursor_to_focused(state);
            }
            emit_workspace_changed(state);
            false
        }
        Action::WorkspaceIndex(n) => {
            let old  = state.grid.focused;
            let cols = state.grid.cols as usize;
            let idx  = n.saturating_sub(1) as usize;
            let col  = (idx % cols) as u8;
            let row  = (idx / cols) as u8;
            state.grid.switch_workspace((col, row));
            apply_workspace_template(state, (col, row), false);
            trigger_slide(state, old);
            sync_focus(state);
            if state.config.input.warp_cursor_on_workspace_change {
                warp_cursor_to_focused(state);
            }
            emit_workspace_changed(state);
            false
        }

        Action::FocusLeft  => { state.grid.move_focus(FocusDirection::Left);  sync_focus(state); emit_focus_changed(state); false }
        Action::FocusRight => { state.grid.move_focus(FocusDirection::Right); sync_focus(state); emit_focus_changed(state); false }
        Action::FocusUp    => { state.grid.move_focus(FocusDirection::Up);    sync_focus(state); emit_focus_changed(state); false }
        Action::FocusDown  => { state.grid.move_focus(FocusDirection::Down);  sync_focus(state); emit_focus_changed(state); false }

        Action::StateFullscreenToggle => {
            with_move_anim(state, |s| s.grid.toggle_fullscreen());
            state.wlr_update_all_states();
            emit_window_state_slot_changed(state);
            false
        }
        Action::StateTotalFullscreenToggle => {
            // Check state before toggle to determine direction for power-profile hook.
            let was_tf = state.grid.focused_window
                .and_then(|id| state.grid.windows.get(&id))
                .map(|w| w.current_state == crate::grid::window::WindowState::TotalFullscreen)
                .unwrap_or(false);
            with_move_anim(state, |s| s.grid.toggle_total_fullscreen());
            state.wlr_update_all_states();
            emit_window_state_slot_changed(state);
            // §22.17 — power-profile hook.
            if state.config.fullscreen.performance_on_total_fullscreen {
                if was_tf {
                    exec_cmd("powerprofilesctl set balanced");
                } else {
                    exec_cmd("powerprofilesctl set performance");
                }
            }
            false
        }
        Action::StateFloatingToggle => {
            with_move_anim(state, |s| s.grid.toggle_floating());
            state.wlr_update_all_states();
            emit_window_state_slot_changed(state);
            false
        }

        Action::SlotHalfLeft   => { with_move_anim(state, |s| s.grid.assign_slot(Slot::HalfLeft));   emit_window_state_slot_changed(state); false }
        Action::SlotHalfRight  => { with_move_anim(state, |s| s.grid.assign_slot(Slot::HalfRight));  emit_window_state_slot_changed(state); false }
        Action::SlotQuarterTL  => { with_move_anim(state, |s| s.grid.assign_slot(Slot::QuarterTL));  emit_window_state_slot_changed(state); false }
        Action::SlotQuarterTR  => { with_move_anim(state, |s| s.grid.assign_slot(Slot::QuarterTR));  emit_window_state_slot_changed(state); false }
        Action::SlotQuarterBL  => { with_move_anim(state, |s| s.grid.assign_slot(Slot::QuarterBL));  emit_window_state_slot_changed(state); false }
        Action::SlotQuarterBR  => { with_move_anim(state, |s| s.grid.assign_slot(Slot::QuarterBR));  emit_window_state_slot_changed(state); false }

        Action::StackNext      => { state.grid.stack_next();    sync_focus(state); emit_stack_changed(state); false }
        Action::StackPrev      => { state.grid.stack_prev();    sync_focus(state); emit_stack_changed(state); false }
        Action::StackFlip(n)   => { state.grid.stack_flip(n);   sync_focus(state); emit_stack_changed(state); emit_stack_reordered(state); false }
        Action::StackPromote   => { state.grid.stack_promote(); sync_focus(state); emit_stack_changed(state); false }
        Action::StackCollapse  => {
            with_move_anim(state, |s| s.grid.stack_collapse());
            sync_focus(state);
            emit_stack_changed(state);
            false
        }
        Action::StackMoveUp    => { state.grid.stack_move_up();   sync_focus(state); emit_stack_changed(state); emit_stack_reordered(state); false }
        Action::StackMoveDown  => { state.grid.stack_move_down(); sync_focus(state); emit_stack_changed(state); emit_stack_reordered(state); false }

        Action::MoveWindowDirection(dir) => {
            let win_id = state.grid.focused_window;
            // Snapshot rects, run the move, detect skips, emit events.
            let duration_ms = state.anim_duration(state.config.view.slide_duration_ms as u64 / 2);
            let before: Vec<_> = state.grid.visible_windows()
                .into_iter()
                .map(|(w, r)| (w.id, r))
                .collect();
            let result = state.grid.move_window_direction_ex(dir);
            if duration_ms > 0 {
                for (id, old_rect) in before {
                    if let Some(new_rect) = state.grid.compute_rect(id) {
                        if old_rect != new_rect {
                            state.move_anims.insert(id, MoveAnim::new(old_rect, duration_ms));
                        }
                    }
                }
            }
            if let Some(id) = win_id {
                match result {
                    Some(MoveDirectionResult::Moved { requested, actual, skipped: true }) => {
                        state.pending_events.push(Event::WindowMoveSkipped {
                            id,
                            requested_col: requested.0,
                            requested_row: requested.1,
                            actual_col: actual.0,
                            actual_row: actual.1,
                        });
                    }
                    Some(MoveDirectionResult::Refused) => {
                        state.pending_events.push(Event::WindowMoveRefused {
                            id,
                            requested_col: state.grid.focused.0,
                            requested_row: state.grid.focused.1,
                            reason: "all-protected".into(),
                        });
                    }
                    _ => {}
                }
            }
            sync_focus(state);
            false
        }
        Action::MoveWindowToIndex(n) => {
            let cols = state.grid.cols as usize;
            let idx  = n.saturating_sub(1) as usize;
            let col  = (idx % cols) as u8;
            let row  = (idx / cols) as u8;
            if let Some(id) = state.grid.focused_window {
                if move_to_tf_protected(state, id, col, row) {
                    return false;
                }
                with_move_anim(state, |s| s.grid.move_window_to((col, row)));
                state.pending_events.push(Event::WindowMoved { id, col, row });
            }
            sync_focus(state);
            false
        }
        Action::MoveWindowTo(col, row) => {
            if let Some(id) = state.grid.focused_window {
                if move_to_tf_protected(state, id, col, row) {
                    return false;
                }
                with_move_anim(state, |s| s.grid.move_window_to((col, row)));
                state.pending_events.push(Event::WindowMoved { id, col, row });
            }
            sync_focus(state);
            false
        }

        Action::ResizeActive(dx, dy) => {
            // Resize the focused floating window.
            if let Some(id) = state.grid.focused_window {
                if let Some(w) = state.grid.windows.get_mut(&id) {
                    if w.current_state == crate::grid::window::WindowState::Floating {
                        w.floating_geom.w = (w.floating_geom.w + dx).max(100);
                        w.floating_geom.h = (w.floating_geom.h + dy).max(60);
                        let id = id;
                        state.grid.send_configure(id);
                    }
                }
            }
            false
        }

        Action::TotalFullscreenExit => {
            // Exit total-fullscreen only if the focused window is in that state.
            let was_tf = state.grid.focused_window
                .and_then(|id| state.grid.windows.get(&id))
                .map(|w| w.current_state == crate::grid::window::WindowState::TotalFullscreen)
                .unwrap_or(false);
            if was_tf {
                with_move_anim(state, |s| s.grid.toggle_total_fullscreen());
                state.wlr_update_all_states();
                if state.config.fullscreen.performance_on_total_fullscreen {
                    exec_cmd("powerprofilesctl set balanced");
                }
            }
            false
        }

        Action::WorkspaceSync => {
            state.workspace_synced = !state.workspace_synced;
            state.pending_events.push(Event::WorkspaceSyncChanged { synced: state.workspace_synced });
            tracing::debug!(synced = state.workspace_synced, "workspace-sync toggled");
            false
        }

        Action::WorkspaceRename(col, row, name) => {
            let old_name = state.grid.workspace_name(col, row).to_owned();
            state.grid.rename_workspace(col, row, name.clone());
            state.pending_events.push(Event::WorkspaceRenamed { col, row, old: old_name, new: name });
            false
        }

        Action::MinimapToggle => {
            state.minimap_visible = !state.minimap_visible;
            state.pending_events.push(Event::MinimapToggled { visible: state.minimap_visible });
            tracing::debug!(visible = state.minimap_visible, "minimap toggled");
            false
        }

        Action::MoveWindowToWorkspaceAndFollow(col, row) => {
            with_move_anim(state, |s| s.grid.move_window_to((col, row)));
            // Switch to the target workspace to follow the window.
            let old = state.grid.focused;
            state.grid.switch_workspace((col, row));
            trigger_slide(state, old);
            sync_focus(state);
            emit_workspace_changed(state);
            false
        }

        Action::SpawnFloating(cmd) => {
            state.spawn_hint = Some(PlacementHints {
                state: Some(crate::grid::window::WindowState::Floating),
                ..Default::default()
            });
            exec_cmd(&cmd);
            false
        }

        Action::SpawnInSlot(slot, cmd) => {
            state.spawn_hint = Some(PlacementHints {
                slot: Some(slot),
                ..Default::default()
            });
            exec_cmd(&cmd);
            false
        }

        Action::SpawnOnWorkspace(col, row, cmd) => {
            state.spawn_hint = Some(PlacementHints {
                workspace: Some((col, row)),
                ..Default::default()
            });
            exec_cmd(&cmd);
            false
        }

        Action::SpawnStacked(cmd) => {
            // Spawn into the currently focused slot's stack (if any).
            let slot = state.grid.focused_window
                .and_then(|id| state.grid.windows.get(&id))
                .and_then(|w| w.current_slot);
            state.spawn_hint = Some(PlacementHints {
                slot,
                ..Default::default()
            });
            exec_cmd(&cmd);
            false
        }

        Action::Submap(name) => {
            if state.submap_tables.contains_key(&name) {
                tracing::debug!(submap = %name, "Entering submap");
                state.active_submap = Some(name.clone());
                state.pending_events.push(Event::SubMapChanged { name: name.clone() });
                state.show_osd("submap");
            } else {
                tracing::warn!(submap = %name, "Unknown submap — ignoring");
            }
            false
        }
        Action::SubmapReset => {
            if let Some(ref name) = state.active_submap {
                tracing::debug!(submap = %name, "Exiting submap");
            }
            state.active_submap = None;
            state.pending_events.push(Event::SubMapChanged { name: String::new() });
            // Hide OSD on submap exit.
            state.osd_hide_at = None;
            state.osd_kind = None;
            false
        }
        Action::SubmapActionThenReset(inner) => {
            state.active_submap = None;
            dispatch(*inner, state)
        }

        Action::OverviewToggle => {
            state.is_overview = !state.is_overview;
            if state.is_overview {
                // Keyboard focus starts at the currently active workspace.
                state.overview_focused = state.grid.focused;
                state.overview_window_idx = 0;
                state.pending_events.push(Event::OverviewOpened);
                state.pending_events.push(Event::ViewModeChanged { mode: "overview".into() });
            } else {
                // Cancel any active window grab when exiting overview.
                if let Some(win_id) = state.overview_grabbed_window.take() {
                    if let Some(origin) = state.overview_grab_origin.take() {
                        state.grid.focused_window = Some(win_id);
                        with_move_anim(state, |s| s.grid.move_window_to(origin));
                    }
                }
                state.overview_window_idx = 0;
                state.pending_events.push(Event::OverviewClosed);
                state.pending_events.push(Event::ViewModeChanged { mode: "focus".into() });
            }
            false
        }
        Action::OverviewFocusLeft => {
            if state.is_overview {
                let new_col = state.overview_focused.0.saturating_sub(1);
                overview_move_focus(state, new_col, state.overview_focused.1);
            }
            false
        }
        Action::OverviewFocusRight => {
            if state.is_overview {
                let cols = state.grid.cols;
                let new_col = (state.overview_focused.0 + 1).min(cols - 1);
                overview_move_focus(state, new_col, state.overview_focused.1);
            }
            false
        }
        Action::OverviewFocusUp => {
            if state.is_overview {
                let new_row = state.overview_focused.1.saturating_sub(1);
                overview_move_focus(state, state.overview_focused.0, new_row);
            }
            false
        }
        Action::OverviewFocusDown => {
            if state.is_overview {
                let rows = state.grid.rows;
                let new_row = (state.overview_focused.1 + 1).min(rows - 1);
                overview_move_focus(state, state.overview_focused.0, new_row);
            }
            false
        }
        Action::OverviewActivate => {
            if state.is_overview {
                // If a window is grabbed, drop it on the focused workspace.
                if let Some(win_id) = state.overview_grabbed_window.take() {
                    let target = state.overview_focused;
                    state.overview_grab_origin = None;
                    state.grid.focused_window = Some(win_id);
                    with_move_anim(state, |s| s.grid.move_window_to(target));
                    // Stay in overview; the window is now in the target thumbnail.
                    return false;
                }
                let target = state.overview_focused;
                // Capture which window in the thumbnail is Tab-selected.
                let win_idx = state.overview_window_idx;
                let selected_id = {
                    let ws = state.grid.visible_windows_for_ws(target);
                    ws.get(win_idx).map(|(w, _)| w.id)
                };
                // Exit overview, navigate to workspace, focus selected window.
                state.is_overview = false;
                state.overview_window_idx = 0;
                state.pending_events.push(Event::OverviewClosed);
                state.pending_events.push(Event::ViewModeChanged { mode: "focus".into() });
                dispatch(Action::WorkspaceTo(target.0, target.1), state);
                // Override focus to the Tab-selected window if set.
                if let Some(id) = selected_id {
                    state.grid.focused_window = Some(id);
                    sync_focus(state);
                }
            }
            false
        }
        Action::OverviewWindowNext => {
            if state.is_overview {
                let ws = state.grid.visible_windows_for_ws(state.overview_focused);
                if !ws.is_empty() {
                    state.overview_window_idx = (state.overview_window_idx + 1) % ws.len();
                }
            }
            false
        }
        Action::OverviewWindowPrev => {
            if state.is_overview {
                let ws = state.grid.visible_windows_for_ws(state.overview_focused);
                if !ws.is_empty() {
                    let n = ws.len();
                    state.overview_window_idx = (state.overview_window_idx + n - 1) % n;
                }
            }
            false
        }
        Action::OverviewGrabWindow => {
            if state.is_overview && state.overview_grabbed_window.is_none() {
                let ws = state.grid.visible_windows_for_ws(state.overview_focused);
                let idx = state.overview_window_idx.min(ws.len().saturating_sub(1));
                if let Some((w, _)) = ws.get(idx) {
                    state.overview_grabbed_window = Some(w.id);
                    state.overview_grab_origin = Some(state.overview_focused);
                    tracing::debug!(id = w.id, "Overview: grabbed window");
                }
            }
            false
        }
        Action::OverviewDropWindow => {
            // Same as OverviewActivate when a grab is active.
            dispatch(Action::OverviewActivate, state)
        }
        Action::OverviewCancelGrab => {
            if state.is_overview {
                if let Some(win_id) = state.overview_grabbed_window.take() {
                    if let Some(origin) = state.overview_grab_origin.take() {
                        // Return the grabbed window to its origin workspace.
                        state.grid.focused_window = Some(win_id);
                        with_move_anim(state, |s| s.grid.move_window_to(origin));
                    }
                }
            }
            false
        }
        Action::Screenshot(mode) => {
            // Delegate to grim/slurp if available; warn if not.
            let cmd = match mode.as_str() {
                "region" => "grim -g \"$(slurp)\" - | wl-copy",
                "screen" => "grim - | wl-copy",
                _        => "grim - | wl-copy",
            };
            exec_cmd(cmd);
            false
        }

        Action::ApplyTemplate(col, row) => {
            let (col, row) = if col == u8::MAX && row == u8::MAX {
                state.grid.focused
            } else {
                (col, row)
            };
            apply_workspace_template(state, (col, row), true);
            false
        }

        Action::ToggleSpecial(name) => {
            if state.active_special.as_deref() == Some(name.as_str()) {
                // Hide: return visible windows from this special back to their last ws.
                state.active_special = None;
                let focused_ws = state.grid.focused;
                if let Some(ids) = state.special_workspaces.get(&name).cloned() {
                    for id in ids {
                        state.grid.attach_window_from_special(id, focused_ws);
                    }
                }
            } else {
                // Show: bring windows from the named special into view.
                state.active_special = Some(name.clone());
                // If this special has no windows yet, initialize the slot.
                state.special_workspaces.entry(name).or_default();
            }
            sync_focus(state);
            false
        }

        Action::MoveToSpecial(name) => {
            if let Some(win_id) = state.grid.focused_window {
                state.grid.detach_window_to_special(win_id);
                state.special_workspaces.entry(name).or_default().push(win_id);
                sync_focus(state);
            }
            false
        }

        Action::OverviewPeek => {
            // Toggle transient peek-overview. Unlike OverviewToggle, peek tracks
            // is_overview_peeking so release-binds can exit without touching the
            // persistent overview toggle state.
            state.is_overview_peeking = !state.is_overview_peeking;
            if state.is_overview_peeking {
                state.overview_focused = state.grid.focused;
                state.overview_window_idx = 0;
                state.pending_events.push(Event::ViewModeChanged { mode: "overview".into() });
            } else {
                state.pending_events.push(Event::ViewModeChanged { mode: "focus".into() });
            }
            false
        }

        Action::StackPeek => {
            // Toggle transient stack-peek fan-out for the focused slot.
            state.is_stack_peeking = !state.is_stack_peeking;
            tracing::debug!(peeking = state.is_stack_peeking, "stack-peek toggled");
            false
        }

        Action::SetWindowShader(id, path) => {
            if let Some(w) = state.grid.windows.get_mut(&id) {
                w.shader = if path.is_empty() || path == "clear" { None } else { Some(path.clone()) };
                let path_ref = w.shader.as_deref().unwrap_or("(cleared)");
                state.pending_events.push(Event::ShaderLoaded { category: "window".into(), path: path_ref.into() });
                tracing::info!(id, path = path_ref, "set-window-shader");
            } else {
                tracing::warn!(id, "set-window-shader: window not found");
            }
            false
        }

        Action::ScreenShader(path) => {
            if path.is_empty() || path == "clear" {
                state.screen_shader = None;
                tracing::info!("screen-shader cleared");
            } else {
                state.screen_shader = Some(path.clone());
                state.pending_events.push(Event::ShaderLoaded { category: "screen".into(), path: path.clone() });
                tracing::info!(path, "screen-shader set");
            }
            false
        }
    }
}

/// Update keyboard focus to match the grid's focused window.
pub fn sync_focus(state: &mut GlobalState) {
    if let Some(id) = state.grid.focused_window {
        if let Some(w) = state.grid.windows.get_mut(&id) {
            w.is_urgent = false;
        }
    }
    if let Some(keyboard) = state.seat.get_keyboard() {
        let keyboard = keyboard.clone();
        let surface = state.grid.focused_surface().cloned();
        keyboard.set_focus(state, surface, Serial::from(0u32));
    }
    if state.config.input.warp_cursor_on_focus_change {
        warp_cursor_to_focused(state);
    }
}

/// Warp the pointer to the center of the currently focused window (§22.5).
pub fn warp_cursor_to_focused(state: &mut GlobalState) {
    let Some(id) = state.grid.focused_window else { return };
    let Some(rect) = state.grid.compute_rect(id) else { return };
    let cx = rect.x + rect.w / 2;
    let cy = rect.y + rect.h / 2;
    state.cursor_pos = (cx as f64, cy as f64);
    if let Some(pointer) = state.seat.get_pointer() {
        let pointer = pointer.clone();
        pointer.motion(
            state,
            None,
            &smithay::input::pointer::MotionEvent {
                location: (cx as f64, cy as f64).into(),
                serial: SERIAL_COUNTER.next_serial(),
                time: 0,
            },
        );
    }
}

fn exec_cmd(cmd: &str) {
    let mut parts = cmd.split_whitespace();
    if let Some(prog) = parts.next() {
        let args: Vec<&str> = parts.collect();
        match std::process::Command::new(prog).args(&args).spawn() {
            Ok(child) => tracing::info!(pid = child.id(), "Spawned {}", prog),
            Err(e)    => tracing::warn!("Failed to spawn {}: {}", prog, e),
        }
    }
}

/// Returns true if the move was refused because target workspace is TF-protected (§6.7.1).
/// Emits `window_move_refused` when refusing. TotalFullscreen and Floating movers are exempt.
fn move_to_tf_protected(state: &mut GlobalState, id: u64, col: u8, row: u8) -> bool {
    let mover_state = state.grid.windows.get(&id).map(|w| w.current_state);
    let Some(ms) = mover_state else { return false };
    use crate::grid::window::WindowState;
    if matches!(ms, WindowState::TotalFullscreen | WindowState::Floating) {
        return false; // always allowed
    }
    if state.grid.workspace_has_total_fullscreen((col, row)) {
        state.pending_events.push(Event::WindowMoveRefused {
            id,
            requested_col: col,
            requested_row: row,
            reason: "tf-protected".into(),
        });
        return true;
    }
    false
}

fn emit_window_state_slot_changed(state: &mut GlobalState) {
    let Some(id) = state.grid.focused_window else { return };
    let Some(w) = state.grid.windows.get(&id) else { return };
    let state_str = crate::ipc::commands::state_name(w.current_state).to_owned();
    let slot_str = w.current_slot
        .map(crate::ipc::commands::slot_name)
        .unwrap_or("")
        .to_owned();
    state.pending_events.push(Event::WindowStateChanged { id, state: state_str });
    if !slot_str.is_empty() {
        state.pending_events.push(Event::WindowSlotChanged { id, slot: slot_str });
    }
}

fn emit_stack_changed(state: &mut GlobalState) {
    let Some(id) = state.grid.focused_window else { return };
    let Some(w) = state.grid.windows.get(&id) else { return };
    let Some(slot) = w.current_slot else { return };
    let ws = state.grid.focused;
    let stack_size = state.grid.slot_stack_size(ws, slot);
    let slot_name = crate::ipc::commands::slot_name(slot).to_owned();
    state.pending_events.push(Event::WindowStackChanged {
        slot: slot_name,
        top_id: id,
        size: stack_size,
    });
}

fn emit_stack_reordered(state: &mut GlobalState) {
    let Some(id) = state.grid.focused_window else { return };
    let Some(w) = state.grid.windows.get(&id) else { return };
    let Some(slot) = w.current_slot else { return };
    let ws = w.workspace;
    let ids = state.grid.slot_stack_ids(ws, slot);
    if ids.len() < 2 { return; } // single-element stack can't be reordered
    let order_csv = ids.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
    let slot_name = crate::ipc::commands::slot_name(slot).to_owned();
    state.pending_events.push(Event::WindowStackReordered { slot: slot_name, order_csv });
}

fn emit_workspace_changed(state: &mut GlobalState) {
    let (col, row) = state.grid.focused;
    state.pending_events.push(Event::WorkspaceChanged { col, row });
    // Show OSD workspace indicator (§8.9).
    state.show_osd("workspace");
    // Notify ext-workspace-v1 and wlr-foreign-toplevel-management clients.
    state.ext_workspace_update_all();
    state.wlr_update_all_states();
}

fn emit_focus_changed(state: &mut GlobalState) {
    if let Some(id) = state.grid.focused_window {
        state.pending_events.push(Event::WindowFocus { id });
    }
    state.wlr_update_all_states();
}

/// Navigate the grid in `dir`, start a slide animation, sync focus, emit event.
fn nav_workspace(state: &mut GlobalState, dir: FocusDirection) {
    let old = state.grid.focused;
    state.grid.navigate(dir);
    let target = state.grid.focused;
    apply_workspace_template(state, target, false);
    trigger_slide(state, old);
    sync_focus(state);
    if state.config.input.warp_cursor_on_workspace_change {
        warp_cursor_to_focused(state);
    }
    emit_workspace_changed(state);
}

/// Snapshot the focused window's rect, run `f`, then start a move animation if
/// the rect changed. Handles multiple windows affected by conflict resolution by
/// snapshotting all visible windows' rects before and after.
fn with_move_anim(state: &mut GlobalState, f: impl FnOnce(&mut GlobalState)) {
    let duration_ms = state.anim_duration(state.config.view.slide_duration_ms as u64 / 2);
    if duration_ms == 0 {
        f(state);
        return;
    }

    // Snapshot rects of all windows on the focused workspace before the action.
    let before: Vec<(crate::grid::window::WindowId, crate::grid::window::Rect)> = {
        state.grid.visible_windows()
            .into_iter()
            .map(|(w, r)| (w.id, r))
            .collect()
    };

    f(state);

    // For each window that moved, start a move animation from its old rect.
    for (id, old_rect) in before {
        if let Some(new_rect) = state.grid.compute_rect(id) {
            if old_rect != new_rect {
                state.move_anims.insert(id, MoveAnim::new(old_rect, duration_ms));
            }
        }
    }
}

/// Navigate diagonally by (dc, dr) steps — both axes move simultaneously (§5).
/// Each axis wraps independently if `grid.wrap_x` / `grid.wrap_y` is set.
/// If either axis can't move (at edge, no wrap), the whole diagonal move aborts.
fn nav_workspace_diagonal(state: &mut GlobalState, dc: i8, dr: i8) {
    let (col, row) = state.grid.focused;
    let cols = state.grid.cols;
    let rows = state.grid.rows;

    let new_col = apply_delta(col, dc, cols, state.grid.wrap_x());
    let new_row = apply_delta(row, dr, rows, state.grid.wrap_y());

    if let (Some(c), Some(r)) = (new_col, new_row) {
        let old = state.grid.focused;
        state.grid.switch_workspace((c, r));
        apply_workspace_template(state, (c, r), false);
        trigger_slide(state, old);
        sync_focus(state);
        emit_workspace_changed(state);
    }
}

fn apply_delta(pos: u8, delta: i8, size: u8, wrap: bool) -> Option<u8> {
    let new = pos as i16 + delta as i16;
    if new >= 0 && new < size as i16 {
        Some(new as u8)
    } else if wrap {
        Some(new.rem_euclid(size as i16) as u8)
    } else {
        None
    }
}

/// Move the overview keyboard focus to a new thumbnail cell; also move the
/// grabbed window there if one is active (§7.2 grab-window).
fn overview_move_focus(state: &mut GlobalState, col: u8, row: u8) {
    let target = (col, row);
    if target == state.overview_focused { return; }

    if let Some(win_id) = state.overview_grabbed_window {
        // Move the grabbed window to the new workspace as we navigate.
        state.grid.focused_window = Some(win_id);
        with_move_anim(state, |s| s.grid.move_window_to(target));
    }

    state.overview_focused = target;
    // Reset Tab-window selection when switching thumbnails.
    state.overview_window_idx = 0;
}

/// If the workspace actually changed, start (or replace) a slide animation.
fn trigger_slide(state: &mut GlobalState, old: (u8, u8)) {
    let new = state.grid.focused;
    if old == new { return; }
    let dir = animate::slide_direction(old, new);
    let duration_ms = state.anim_duration(state.config.view.slide_duration_ms as u64);
    state.slide_anim = Some(WorkspaceSlide::new(old, dir, duration_ms));
}

/// Apply the workspace template for `coord` if it hasn't been applied yet.
/// Set `force = true` to re-apply even if it was already applied.
///
/// Each window entry in the template is handled in order:
/// 1. If `app_id` is set and a matching window exists anywhere, move it to `coord`
///    with the requested slot assignment.
/// 2. Otherwise, spawn the `exec` command with the requested slot hint.
pub fn apply_workspace_template(state: &mut GlobalState, coord: (u8, u8), force: bool) {
    if !force && state.applied_templates.contains(&coord) {
        return;
    }

    // Find the template for this coord.
    let template = state.templates.templates.iter()
        .find(|t| t.cell == [coord.0, coord.1])
        .cloned();

    let Some(template) = template else { return };

    // Mark as applied (even if empty, so we don't re-check every visit).
    state.applied_templates.insert(coord);

    tracing::info!(
        col = coord.0,
        row = coord.1,
        name = ?template.name,
        windows = template.windows.len(),
        "Applying workspace template"
    );

    for win_cfg in &template.windows {
        let slot = parse_slot(&win_cfg.slot);

        // Try to find a matching existing window by app_id glob.
        let matched_id = win_cfg.app_id.as_deref().and_then(|pattern| {
            state.grid.windows.iter().find_map(|(id, w)| {
                let aid = &w.app_id;
                if glob_matches(pattern, aid) { Some(*id) } else { None }
            })
        });

        if let Some(win_id) = matched_id {
            // Move the existing window to this workspace.
            state.grid.focused_window = Some(win_id);
            state.grid.move_window_to(coord);
            // Assign the slot if specified.
            if let Some(s) = slot {
                state.grid.assign_slot(s);
            }
        } else {
            // Spawn a new window with placement hints.
            let hints = PlacementHints {
                workspace: Some(coord),
                slot,
                ..Default::default()
            };
            state.spawn_hint = Some(hints);
            exec_cmd(&win_cfg.exec);
        }
    }
}

/// Simple glob matcher: `*` matches any sequence, `?` matches one character.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    glob_match_inner(&pattern, &text)
}

fn glob_match_inner(pat: &[char], txt: &[char]) -> bool {
    match (pat.first(), txt.first()) {
        (None, None)              => true,
        (Some(&'*'), _)           => {
            // '*' matches zero characters or one character.
            glob_match_inner(&pat[1..], txt)
                || (!txt.is_empty() && glob_match_inner(pat, &txt[1..]))
        }
        (Some(&'?'), Some(_))     => glob_match_inner(&pat[1..], &txt[1..]),
        (Some(p), Some(t)) if p == t => glob_match_inner(&pat[1..], &txt[1..]),
        _                         => false,
    }
}
