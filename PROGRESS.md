# GriddyWM Build Progress Log

## Session 1 — 2026-05-21

### What was built

**Phase 0 — Foundation: COMPLETE**

Initialized a Cargo workspace with two crates:
- `griddy/` — the compositor binary
- `griddyctl/` — the control CLI

#### Files created

```
Cargo.toml                          workspace root
griddy/Cargo.toml                   compositor crate
griddy/src/main.rs                  entry point: CLI args (clap), logging (tracing), event loop init
griddy/src/state.rs                 GlobalState struct + ClientData (per-connection)
griddy/src/config/mod.rs            config loader: XDG path search, file imports, apply_env()
griddy/src/config/types.rs          full TOML config structs (Grid, View, Input, Windows, Startup, etc.)
griddy/src/handlers/mod.rs          module declarations
griddy/src/handlers/compositor.rs   CompositorHandler + ShmHandler + BufferHandler
griddy/src/handlers/xdg_shell.rs    XdgShellHandler (new_toplevel, fullscreen, move, resize stubs)
griddy/src/handlers/seat.rs         SeatHandler (keyboard + pointer focus)
griddy/src/handlers/data_device.rs  DataDeviceHandler + SelectionHandler
griddy/src/backend/mod.rs           BackendType enum + run_backend()
griddy/src/backend/winit.rs         winit dev backend: render loop, input, Wayland socket
griddyctl/Cargo.toml                CLI crate
griddyctl/src/main.rs               griddyctl stub: dispatch/get/reload/shader/batch subcommands
```

#### Smithay 0.7.0 — key lessons learned

| Issue | Resolution |
|-------|-----------|
| `smithay-drm-extras` needs `libdisplay-info < 0.3` but Fedora 44 ships 0.3 | Excluded from deps; add back when upstream fixes version req |
| `winit::platform::pump_events::PumpStatus` unreachable from external crates | cfg_aliases in winit's build.rs don't propagate; use a `should_exit: bool` flag instead |
| `WinitGraphicsBackend::bind()` returns `(renderer, framebuffer)` | Must drop `framebuffer` in a sub-scope before calling `backend.submit()` |
| smithay error types don't impl `std::error::Error` | Use `.map_err(|e| anyhow::anyhow!("{e:?}"))` not `.context()` |
| `std::env::set_var` is `unsafe` in Rust 2024 edition | Wrap in `unsafe {}` block |
| `WinitEventLoop::dispatch_new_events` is NOT a calloop EventSource | Poll it manually in the main loop; calloop is dispatched with `Duration::ZERO` alongside |

#### Build status

```
cargo build --workspace   →   clean, 2 expected "field unused" warnings
target/debug/griddy       →   163 MB debug binary
target/debug/griddyctl    →   34 MB debug binary
```

---

## Session 2 — 2026-05-21

### What was built

**Phase 1 — Grid, Slots, States: COMPLETE**

Implemented the full window model and workspace grid.

#### Files created

```
griddy/src/grid/mod.rs          Grid struct: window registry, placement, navigation, configure
griddy/src/grid/window.rs       WindowId, Slot, WindowState, Rect, Window
griddy/src/grid/workspace.rs    Workspace: slot stacks, promotion stacks, focus history, Z-order
griddy/src/grid/layout.rs       Slot geometry calculator (Half/Quarter rects, smart gaps)
griddy/src/grid/conflict.rs     Slots-conflict matrix, Half→Quarter adaptation, Fullscreen cascade,
                                  two-pass conflict resolver (stack / swap / kick-to-floating)
griddy/src/grid/placement.rs    New-window placement policy (§6.9): empty→Fullscreen,
                                  1-Fullscreen→split, next-empty-slot scan, stack fallback
griddy/src/grid/focus.rs        Focus-on-close (§6.10), spatial focus navigation (§6.11)
```

#### Files updated

```
griddy/src/main.rs              Added `mod grid`; `#![allow(dead_code)]`
griddy/src/state.rs             Added `grid: Grid` field; initialized in `GlobalState::new`
griddy/src/handlers/xdg_shell.rs  new_toplevel → grid.add_window(); toplevel_destroyed → grid.remove_window()
griddy/src/backend/winit.rs     set_output_size on init/resize; position-aware rendering per slot rect
```

#### Key behaviors implemented

1. **Slot coexistence matrix** (§6.2) — `slots_conflict(a, b)` for all pairs
2. **Half→Quarter adaptation** (§6.5.1) — detects one-free-Quarter and adapts Half into it
3. **Fullscreen cascade** (§6.5.1 steps 1–5) — largest-free-region first, drag target fallback
4. **Conflict resolver** — `stack` (default), `swap`, `kick-to-floating` (§6.5.2)
5. **New-window placement** (§6.9) — empty→Fullscreen, 2nd window auto-splits 50/50
6. **Focus-on-close** (§6.10) — last-focused, stack-next, slot-neighbor, none
7. **Spatial focus navigation** (§6.11) — directional with Euclidean distance ranking
8. **Z-order pipeline** — tiled → Fullscreen → TotalFullscreen → floating (§6.4)
9. **Fullscreen auto-restore** — promotes back when workspace clears (§6.5.1)
10. **Smart gaps** — collapses to zero when workspace has exactly one tiled window
11. **Workspace navigation** — left/right/up/down with wrap, history ring, switch_workspace
12. **Position-aware rendering** — each window rendered at its computed slot Rect

#### Build status

```
cargo build --workspace   →   clean (0 errors, dead_code warnings suppressed)
```

---

## Session 3 — 2026-05-21

### What was built

**Phase 2a — Keybind Dispatcher: COMPLETE**
**Phase 2f — app_id / title tracking: COMPLETE**

#### Files created

```
griddy/src/keybind/mod.rs        BindTable: (modifier-mask, keysym) → Action lookup
griddy/src/keybind/dispatcher.rs Action enum, parse_action(), dispatch(), sync_focus()
```

#### Files updated

```
griddy/src/config/types.rs       Added BindConfig struct; added binds: Vec<BindConfig> to Config
griddy/src/main.rs               Added mod keybind
griddy/src/state.rs              Added keybind_table: BindTable, should_exit: bool
griddy/src/grid/mod.rs           Added 10 new action methods + cross_workspace_focus field
griddy/src/backend/winit.rs      Wired keyboard filter → BindTable → dispatcher
griddy/src/handlers/compositor.rs  commit() reads app_id/title from XdgToplevelSurfaceData
```

#### Key behaviors implemented

1. **Default keybind table** (§9.1): all directional, slot, state, stack, workspace, exec binds
2. **Keybind dispatch flow**: keyboard filter intercepts matched keys, returns Action, dispatches after keyboard.input() returns
3. **Keyboard focus sync** (`sync_focus`): updates keyboard focus after workspace nav / focus move
4. **Grid action methods**: `toggle_fullscreen`, `toggle_total_fullscreen`, `toggle_floating`, `assign_slot`, `move_focus`, `close_focused`, `stack_next`, `stack_prev`, `move_window_direction`, `move_window_to`
5. **app_id / title tracking**: `with_states` + `XdgToplevelSurfaceData` on every surface commit
6. **Quit via keybind**: `Super+Shift+e` sets `state.should_exit = true`, exits main loop
7. **cross_workspace_focus**: spatial focus navigation crosses workspace boundaries (config §6.11)
8. **Workspace-index 1–9** and **move-window-to-index 1–9** binds

#### Smithay notes

| Issue | Resolution |
|-------|-----------|
| `XdgToplevelSurfaceData.current` is `ToplevelState` (no app_id/title) | `app_id`/`title` are top-level fields on `XdgToplevelSurfaceRoleAttributes`, not nested in `.current` |
| `drop(&mut ref)` does nothing with NLL | Wrapped the stack mutation in a block so the mutable borrow is scoped |
| `keyboard.set_focus(state, ...)` borrow conflict | Clone the `KeyboardHandle<D>` Arc first; releases borrow on `state.seat` |

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 4 — 2026-05-22

### What was built

**Phase 2b — Hot-reload: COMPLETE**
**Phase 2c — theme.toml: COMPLETE**
**Phase 2d — rules.toml: COMPLETE**
**Phase 2e — Submaps: COMPLETE**

#### Files created

```
griddy/src/config/theme.rs      ThemeConfig, GapsConfig, WindowGapConfig, WorkspaceGapConfig,
                                  ColorConfig, CursorConfig, BlurConfig
griddy/src/config/rules.rs      Rule, RuleMatch, RuleAction, glob_match()
```

#### Files updated

```
griddy/src/config/types.rs      Added SubBindConfig, SubmapConfig; added theme/rules/submaps to Config
griddy/src/config/mod.rs        Auto-load theme.toml, rules.toml, keybinds.toml from config dir
griddy/src/grid/mod.rs          Added PlacementHints struct; inner_gap_px/outer_gap_px fields;
                                  update_from_config(); add_window(hints) now uses rule overrides;
                                  compute_rect() uses live gap values from config
griddy/src/handlers/xdg_shell.rs  evaluate_rules() → PlacementHints before add_window(); reads
                                   app_id/title before placement so rules can match
griddy/src/keybind/mod.rs       Added SubBindTable; build_submap_tables(); placement_submap();
                                  resize_submap(); added $mod+w and $mod+r to default_binds
griddy/src/keybind/dispatcher.rs  Added Action::Submap, SubmapReset, SubmapActionThenReset,
                                   ResizeActive, CenterFloating; wired ReloadConfig to live reload
griddy/src/state.rs             Added config_path, config_mtime, submap_tables, active_submap;
                                  reload_config_if_changed(); GlobalState::new() takes config_path
griddy/src/backend/winit.rs     Submap-aware keyboard filter (exit_keys, exit_after_action,
                                  exit_on_unhandled); periodic hot-reload check every 500ms
griddy/src/main.rs              Passes resolved config_path to GlobalState::new()
```

#### Key behaviors implemented

1. **Hot-reload (§2b)**: Config checked every 500ms by comparing file mtime. On change: keybind table + submap tables rebuilt, gaps applied to live grid, active submap cleared. On parse error: previous config retained, mtime updated to avoid retry spam. `Super+Shift+r` also forces an immediate reload.

2. **theme.toml (§2c / §8.5)**: `ThemeConfig` with `[gaps.windows]` inner_px/outer_px/smart, `[gaps.workspaces]`, `[colors]`, `[cursor]`, `[blur]`. Auto-loaded from same dir as config.toml. `Grid::update_from_config()` applies new gap values and reconfigures all live windows. `compute_rect()` now reads `grid.inner_gap_px` / `grid.outer_gap_px` instead of hardcoded constants.

3. **rules.toml (§2d / §10)**: `Rule` with `RuleMatch` (glob app_id, title) and `RuleAction` (slot, state, workspace, floating_geom, opacity, no_focus, pin). Glob matcher supports `*` and `?`. Rules evaluated in `new_toplevel()` before `add_window()`, passed as `PlacementHints`. Last-wins cascade.

4. **Submaps (§2e / §8.7)**: 
   - Built-in `placement` submap (`$mod+w`): h/l=half, u/i/j/k=quarters, f/F/v=state toggles. `exit_after_action=true` + `exit_on_unhandled=true` → exits on any key press.
   - Built-in `resize` submap (`$mod+r`): h/l/j/k resize floating window by 20px with repeat. Exits on Escape/Return only.
   - Keyboard filter checks `state.active_submap` first: exit_keys exit, matched actions dispatch (then reset if `exit_after_action`), unhandled keys exit if `exit_on_unhandled`.
   - Action::Submap(name), SubmapReset, SubmapActionThenReset(Box<Action>) added.
   - User-defined submaps via `[[submap]]` in config override built-ins.

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

---

## Session 5 — 2026-05-22

### What was built

**Phase 3 — IPC: COMPLETE**

Dual UNIX socket IPC server wired into the compositor main loop.

#### Files created

```
griddy/src/ipc/mod.rs       IpcServer: non-blocking cmd+evt listeners, accept/broadcast
griddy/src/ipc/events.rs    Event enum + wire format (<name>>><data>\n)
griddy/src/ipc/commands.rs  handle(): dispatch, workspaces, windows, activewindow,
                              activeworkspace, grid, monitors, version, reload, kill, [[BATCH]]
```

#### Files updated

```
griddy/src/main.rs              Added mod ipc
griddy/src/state.rs             Added ipc: Option<IpcServer>, pending_events: Vec<Event>;
                                  IPC server init in GlobalState::new() with GRIDDY_INSTANCE_SIGNATURE
griddy/src/backend/winit.rs     Main loop step 7: accept_event_subscribers(),
                                  try_handle_one() (up to 16/tick), flush_events()
griddy/src/handlers/xdg_shell.rs  Push WindowOpened/WindowClosed into pending_events
griddy/src/keybind/dispatcher.rs  Added WorkspaceTo, MoveWindowTo action variants;
                                  workspace/move-window-to/workspace-direction parse;
                                  emit WorkspaceChanged, WindowFocus, SubMapChanged events
griddyctl/src/main.rs           Added subscribe subcommand (reads .events.sock)
```

#### Key behaviors implemented

1. **Dual sockets** at `$XDG_RUNTIME_DIR/griddy/griddy_<PID>/`:
   - `.command.sock` — request/response; supports `j/` JSON prefix
   - `.events.sock`  — push-only; `griddyctl subscribe` tails it
2. **Command set**: `dispatch`, `workspaces`, `windows`, `activewindow`, `activeworkspace`, `grid`, `monitors`, `version`, `reload`, `kill`, `[[BATCH]]`
3. **Events emitted**: `workspace_changed`, `window_opened`, `window_closed`, `window_focus`, `submap_changed`, `config_reloaded`, `config_error`
4. **New IPC-friendly dispatchers**: `workspace col,row`, `workspace-direction left|right|up|down`, `move-window-to col,row`
5. **GRIDDY_INSTANCE_SIGNATURE** exported to all child processes
6. **Non-blocking design**: sockets polled each main loop tick; IPC never stalls rendering
7. **griddyctl subscribe**: tails the events socket, prints one event per line

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

#### Usage examples

```bash
griddyctl get grid                          # human-readable grid info
griddyctl -j get workspaces                 # JSON workspace list
griddyctl dispatch workspace-left           # navigate left
griddyctl dispatch workspace 1,2            # jump to (1,2)
griddyctl dispatch move-window-to 0,0       # move focused window to (0,0)
griddyctl --batch "dispatch slot-half-left ; dispatch focus-right"
griddyctl subscribe                         # tail event stream
```

---

---

## Session 6 — 2026-05-22

### What was built

**Phase 4 — Renderer & Animations: IN PROGRESS**

#### Files created

```
griddy/src/animate/mod.rs       WorkspaceSlide, WorkspaceAnim: slide animation state machine,
                                  ease_out_cubic/ease_in_out_cubic, slide_direction helper,
                                  WindowAnim (open/close stub)
```

#### Files updated

```
griddy/src/config/theme.rs      Added WindowDecoConfig, WindowUnfocusedDecoConfig,
                                  WindowThemeSection; window: WindowThemeSection field in ThemeConfig
griddy/src/state.rs             Added DragKind, DragState; cursor_pos, drag, slide_anim fields
                                  to GlobalState; imported WorkspaceSlide + WindowId
griddy/src/grid/mod.rs          Added visible_windows_for_ws(ws_coords); window_at(x, y) hit-test
griddy/src/keybind/dispatcher.rs  nav_workspace() / trigger_slide() helpers; all workspace nav
                                   actions route through nav_workspace for slide animation
griddy/src/backend/winit.rs     Major rewrite: two-phase render (pre-build elements before
                                  renderer.render()), border rendering, slide animation,
                                  pointer tracking, $mod+drag for floating windows
griddy/src/main.rs              Added mod animate
```

#### Key behaviors implemented

1. **Workspace slide animation (§7.1)**: `WorkspaceSlide` state machine with `ease_out_cubic`. Old workspace slides off-screen in the direction of travel; new workspace slides in from the opposite side. Duration from `config.view.slide_duration_ms`.

2. **Border rendering**: Two-pass per-window: `frame.clear(border_color, border_rect)` then surface drawn on top leaving `border_px` margin. Focused window uses accent color; unfocused uses `border_idle`. Configurable `border_px` per focus state.

3. **Two-phase render loop** (avoids double-`&mut renderer` borrow): Phase 1 calls `render_elements_from_surface_tree` for all windows before `renderer.render()`. Phase 2 opens frame, clears background, draws borders + surfaces.

4. **Pointer tracking + hit-test**: `cursor_pos` updated on every `PointerMotionAbsolute`. `window_at(x, y)` walks Z-order top→bottom to find topmost window under cursor. Keyboard focus follows mouse when `input.follow_mouse != Off`.

5. **$mod + drag for floating windows**: `$mod+BTN_LEFT` starts a move drag; `$mod+BTN_RIGHT` starts a resize drag. `DragState` stores start geometry so offsets are computed from origin.

6. **Slide animation for two workspaces**: `visible_windows_for_ws(old_ws)` collects windows for the departing workspace; rendered at `old_ws_offset`. Current workspace rendered at `new_ws_offset`.

#### Smithay notes

| Issue | Resolution |
|-------|-----------|
| Double `&mut renderer` borrow | Pre-build `Vec<WaylandSurfaceRenderElement>` in Phase 1 before calling `renderer.render()` in Phase 2 |
| `render_elements_from_surface_tree` returns `Vec` not iterator | Removed `.collect()` call |
| `Rectangle::from_loc_and_size` deprecated | Replaced with `Rectangle::new((x,y).into(), (w,h).into())` |

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 7 — 2026-05-22

### What was built

**Phase 4 continued: Edge snap + unsnap (§6.12)**

#### Files updated

```
griddy/src/state.rs             Added snap_preview: Option<Slot> and started_tiled: bool
                                  to DragState
griddy/src/backend/winit.rs     Edge snap logic: snap_slot_for_cursor(), edge snap preview
                                  rendering (translucent accent overlay), unsnap tiled→floating
                                  when drag distance > unsnap_threshold_px, snap applied on
                                  button release via grid.assign_slot()
```

#### Key behaviors implemented

1. **Edge snap (§6.12)**: While dragging a floating window with `$mod+BTN_LEFT`, cursor proximity to a screen edge computes a `snap_preview` slot (QuarterTL/TR/BL/BR for corners; HalfLeft/HalfRight for center-1/3 of left/right edges). Translucent accent-colored preview drawn at target slot geometry. On mouse release, `grid.assign_slot()` is called to formally place the window.

2. **Unsnap (§6.12 reverse)**: `$mod+BTN_LEFT` on a tiled window starts an "unsnap pending" drag. When cursor travels more than `unsnap_threshold_px` (default 40), the window is promoted to Floating via `toggle_floating()` and the drag continues as a normal floating-move drag.

3. **snap_slot_for_cursor()**: Corners (both edges within threshold) take priority over half-edges. Half-edges only activate when cursor is within the center 1/3 of the perpendicular screen dimension.

4. **Preview rendering**: Semi-transparent (α=0.25) accent-color rectangle drawn at the target slot geometry while snap is pending.

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 8 — 2026-05-22

### What was built

**Phase 4 continued: Animations — window open fade-in, slot move, AnimationsConfig**

#### Files updated

```
griddy/src/config/types.rs      Added AnimationsConfig { open_duration_ms, close_duration_ms,
                                  on_interrupt, crossfade_ms }; added animations field to Config
griddy/src/animate/mod.rs       Added MoveAnim { old_rect, lerp_rect(), is_done() } for
                                  slot/position transition animation
griddy/src/state.rs             Added open_anims: HashMap<WindowId, WindowAnim>;
                                  move_anims: HashMap<WindowId, MoveAnim>; imported MoveAnim
griddy/src/handlers/xdg_shell.rs  Start open fade-in (WindowAnim) on new_toplevel; remove
                                    on toplevel_destroyed
griddy/src/keybind/dispatcher.rs  Added with_move_anim() helper; wraps all slot/state/move
                                   actions so rect changes trigger a MoveAnim
griddy/src/grid/window.rs       Added PartialEq, Eq to Rect derive
griddy/src/backend/winit.rs     Expire move_anims each tick; apply lerp_rect() in render
                                  item collection; pass per-window alpha to
                                  render_elements_from_surface_tree and border color
```

#### Key behaviors implemented

1. **Window fade-in (§11.2)**: When a new toplevel is mapped, a `WindowAnim::open(open_duration_ms)` starts. The `alpha` field is passed to `render_elements_from_surface_tree` and the border color, so the window fades from transparent to fully opaque over the configured duration (default 160ms).

2. **Slot/position move animation**: `with_move_anim()` snapshots all visible windows' rects before a slot-change action (assign_slot, toggle_fullscreen, toggle_floating, move_window_direction, etc.), runs the action, then for each window whose rect changed creates a `MoveAnim` tracking the old rect. During rendering, `MoveAnim::lerp_rect()` eases the window from its old position to its new position using `ease_out_cubic`. Duration = `slide_duration_ms / 2`.

3. **`[animations]` config section**: `open_duration_ms` (160), `close_duration_ms` (120), `on_interrupt` ("snap-then-start"), `crossfade_ms` (60). All serde-defaulted so existing configs still work.

4. **Workspace slide cancellation (snap-then-start)**: The existing `trigger_slide()` naturally implements snap-then-start — it always starts the new animation from `state.grid.focused` (the current logical destination), so interrupting an in-progress slide immediately snaps it to its end state.

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 9 — 2026-05-22

### What was built

**Phase 6 (early): `wlr-layer-shell` support**

#### Files created

```
griddy/src/handlers/layer_shell.rs   WlrLayerShellHandler impl + layer_rect() geometry helper
```

#### Files updated

```
griddy/src/handlers/mod.rs           Added mod layer_shell
griddy/src/state.rs                  Added WlrLayerShellState, MappedLayerSurface struct,
                                       layer_shell_state + layer_surfaces fields,
                                       recompute_usable_area()
griddy/src/grid/mod.rs               Added usable_x/y/w/h fields; set_usable_area();
                                       compute_rect() uses usable area for Tiled + Fullscreen;
                                       set_output_size() resets usable area
griddy/src/backend/winit.rs          Layer surface rendering in Z order: Background → Bottom →
                                       [windows] → Top → Overlay; layer frame callbacks;
                                       dead layer surface cleanup; imported LayerSurfaceCachedState
```

#### Key behaviors implemented

1. **`wlr-layer-shell` protocol** (`zwlr_layer_shell_v1` v4): Apps like waybar, mako, dunst, swaybar, fuzzel, swaybg can now connect as layer surfaces.

2. **Z-order rendering**: Background → Bottom → tiled/floating windows → Top → Overlay, matching the wlr-layer-shell spec.

3. **Exclusive zone handling**: `recompute_usable_area()` reads all mapped layer surfaces, sums their exclusive zone contributions by anchor direction (top/bottom/left/right), and calls `grid.set_usable_area(x, y, w, h)`. Tiled windows are laid out within the usable area, so a 30px waybar at the top means tiled windows start at y=30.

4. **`layer_rect()`**: Computes layer surface geometry from `LayerSurfaceCachedState` (anchor bitflags, size, margins). Handles: stretch across axis (both-edge anchor), single-edge anchor, center fallback.

5. **Fullscreen respects exclusive zones**: `WindowState::Fullscreen` now maps to the usable area. `WindowState::TotalFullscreen` still covers the entire output.

#### Smithay notes

| Issue | Resolution |
|-------|-----------|
| `WlSurface::id()` needs trait in scope | `use wayland_server::Resource;` in layer_shell.rs |

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 10 — 2026-05-22

### What was built

**Phase 6 continued: `wl_output` + `xdg_output_unstable_v1`**

#### Files updated

```
griddy/src/state.rs              Added OutputManagerState, Output; created output with
                                   OutputMode 1920×1080@60; create_global() advertises wl_output
griddy/src/handlers/compositor.rs  Added OutputHandler impl + delegate_output!
griddy/src/backend/winit.rs      On init + resize: update advertised output mode to actual
                                   winit window size
```

#### Key behaviors implemented

1. **`wl_output` global** (`wl_output` v4): compositor advertises output geometry, mode, scale, and transform to all connecting clients. Bars like waybar that query `wl_output` now get a valid response.

2. **`xdg_output_unstable_v1`** (via `OutputManagerState::new_with_xdg_output`): provides logical geometry to clients, required by most modern Wayland bars for correct positioning.

3. **Live resize updates**: when the winit window changes size, `output.change_current_state()` broadcasts the new mode to all bound `wl_output` instances.

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 11 — 2026-05-22

### What was built

**Phase 6 continued: `xdg-decoration`, `ext-foreign-toplevel-list-v1`**

#### Files updated

```
griddy/src/state.rs              Added XdgDecorationState, ForeignToplevelListState,
                                   toplevel_handles: HashMap<WindowId, ForeignToplevelHandle>
griddy/src/handlers/xdg_shell.rs XdgDecorationHandler (always ServerSide), delegate_xdg_decoration!;
                                   ForeignToplevelListHandler, delegate_foreign_toplevel_list!;
                                   new_toplevel: creates handle; toplevel_destroyed: removes handle
griddy/src/handlers/compositor.rs On commit: send_app_id/send_title/send_done to toplevel handle
```

#### Key behaviors implemented

1. **`xdg-decoration-unstable-v1`**: GriddyWM always responds `ServerSide`. Clients that request CSD still get overridden. This tells apps (GTK, Qt, Electron) not to draw their own titlebars/chrome — our border rendering handles decoration.

2. **`ext-foreign-toplevel-list-v1`**: Every mapped toplevel gets a `ForeignToplevelHandle` registered with `ForeignToplevelListState`. Title/app_id updates are sent on every surface commit. Handle is removed when the window is destroyed. Clients subscribing to this protocol (future taskbars, window switchers) receive a live window list.

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

---

## Session 12 — 2026-05-22

### What was built

**Phase 5 — Overview & Minimap: COMPLETE (initial)**

Overview mode (§7.2) — the zoomed-out workspace grid view.

#### Files created

None.

#### Files updated

```
griddy/src/grid/layout.rs        Added overview_thumbnail_rect() and minimap_cell_rect()
griddy/src/state.rs              Added is_overview: bool, overview_focused: (u8, u8)
griddy/src/ipc/events.rs         Added OverviewOpened, OverviewClosed, ViewModeChanged events
griddy/src/keybind/dispatcher.rs  Added OverviewToggle, OverviewFocusLeft/Right/Up/Down,
                                   OverviewActivate actions + parse_action entries + dispatch cases
griddy/src/keybind/mod.rs        Added $mod+o → overview-toggle default bind
griddy/src/backend/winit.rs      Overview rendering (thumbnail tiles with colored window rects),
                                   keyboard intercept (arrows, Enter, Escape in overview),
                                   click-to-activate workspace in overview,
                                   frame callbacks for all workspaces in overview,
                                   Minimap HUD rendering (corner widget, focus mode only),
                                   lighten() color helper, OV_/MM_ constants, XKB_KEY_ constants
```

#### Key behaviors implemented

1. **Overview toggle** (`$mod+o`): Enters/exits the zoomed-out grid. Entering sets `overview_focused` to the current workspace.

2. **Overview rendering** (§7.2): All workspaces rendered as thumbnail tiles using `overview_thumbnail_rect()`. Each tile shows:
   - Thumbnail background (`bg_alt`) — slightly lighter for the home workspace
   - Colored rectangles per window proportional to their slot positions
   - Window colors: `accent` = focused window, `accent_dim` = tiled, `fg_dim` = floating
   - Border: `accent` (3px) for keyboard-focused thumbnail, `accent_dim` for home workspace, `idle` for others
   - Layer:Top + Overlay surfaces rendered over the overview (so waybar stays visible)

3. **Keyboard navigation** (§7.2): In overview mode, arrow keys are intercepted before normal keybind lookup:
   - `Left/Right/Up/Down` → move `overview_focused` through the grid
   - `Enter` / `KP_Enter` → `OverviewActivate` (switch to focused thumbnail, exit overview)
   - `Escape` → `OverviewToggle` (exit without switching)
   - Normal binds like `$mod+o` still fire (so toggle works to exit)

4. **Click to activate** (§7.2): Click on any workspace thumbnail → switches to that workspace + exits overview. Click outside all thumbnails: consumed (no-op).

5. **Frame callbacks in overview**: All windows across all workspaces receive frame callbacks during overview, preventing clients from stalling.

6. **IPC events**: `overview_opened`, `overview_closed`, `view_mode_changed` (mode: `focus`|`overview`).

7. **Minimap HUD** (§7.4): Small corner widget in focus mode (bottom-right, 16px margin).
   - Each cell: 14×14px with 3px gap
   - Focused workspace: `accent` color
   - Occupied workspaces: `idle` color
   - Empty workspaces: `bg_alt` color
   - Not rendered during overview (redundant)

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 13 — 2026-05-22

### What was built

**Phase 6 continued: Ancillary protocols + pointer event forwarding**

#### Files created

```
griddy/src/handlers/protocols.rs   cursor-shape, presentation-time, idle-inhibit,
                                    xdg-activation, relative-pointer, pointer-constraints
                                    handler impls + delegation macros
```

#### Files updated

```
griddy/src/state.rs        Added cursor_shape_state, presentation_state,
                             idle_inhibit_state, xdg_activation_state,
                             relative_pointer_state, pointer_constraints_state,
                             idle_inhibit_count, frame_seq
griddy/src/handlers/mod.rs  Added pub mod protocols
griddy/src/backend/winit.rs  Presentation-time feedback drain after frame callbacks;
                               Pointer motion/button/axis forwarding to Wayland clients
                               via seat PointerHandle; relative motion events for
                               zwp_relative_pointer_v1; pointer_focus_for_cursor() helper
```

#### Key behaviors implemented

1. **`cursor-shape-v1`**: Browsers, Electron apps, Qt/GTK apps that request cursor shapes
   (pointer, text, crosshair, resize handles, etc.) are now honoured. Routes through
   `SeatHandler::cursor_image` → `TabletSeatHandler` no-op impl.

2. **`presentation-time` (wp_presentation)**: After each frame, all `PresentationFeedbackCachedState`
   callbacks are drained and marked as presented with CLOCK_MONOTONIC timestamp and 60 Hz refresh.
   Firefox, Chromium, mpv, and game engines use this for smooth frame pacing.

3. **`idle-inhibit-unstable-v1`**: mpv, browsers (fullscreen video), games can now inhibit
   the idle timer. `idle_inhibit_count` tracks active inhibitors.

4. **`xdg-activation-v1`**: Apps can request surface activation with a token. Token is
   consumed immediately on `request_activation`; the window's workspace is switched to
   and keyboard focus is set.

5. **`relative-pointer-v1`**: Relative pointer motion events sent via
   `PointerHandle::relative_motion()` on every `PointerMotionAbsolute` event. Games and
   virtual-machine display clients receive correct delta values.

6. **`pointer-constraints-v1`**: Global registered. `new_constraint` activates the lock or
   confine immediately if the constrained surface has keyboard focus. `cursor_position_hint`
   is a no-op (winit dev backend can't capture OS cursor).

7. **Pointer event forwarding** (new): Previously pointer motion, button clicks, and
   scroll events were only processed internally (drag, focus, etc.) but never forwarded
   to Wayland clients. Now `PointerHandle::motion`, `::button`, `::axis`, `::frame`, and
   `::relative_motion` are called on every input event so apps receive their mouse input.

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 14 — 2026-05-22

### What was built

**Phase 6 continued: `ext-workspace-v1` + `wlr-foreign-toplevel-management-unstable-v1`**

#### Files created

```
griddy/src/handlers/workspace_protocol.rs   ext-workspace-v1: workspace grid exposed to shell clients
griddy/src/handlers/foreign_toplevel.rs     wlr-foreign-toplevel-management-unstable-v1: taskbar window list
```

#### Files updated

```
griddy/src/handlers/mod.rs           Added pub mod workspace_protocol, pub mod foreign_toplevel
griddy/src/state.rs                  Added WlrForeignToplevelState + ExtWorkspaceState fields;
                                       registered both globals in GlobalState::new()
griddy/src/handlers/xdg_shell.rs     new_toplevel: wlr_new_toplevel(); toplevel_destroyed: wlr_remove_toplevel()
griddy/src/handlers/compositor.rs    commit(): wlr_update_title() + wlr_update_app_id() on metadata change
griddy/src/keybind/dispatcher.rs     emit_workspace_changed: ext_workspace_update_all() + wlr_update_all_states()
                                       emit_focus_changed: wlr_update_all_states()
                                       Fullscreen/Float toggles: wlr_update_all_states()
```

#### Key behaviors implemented

1. **`ext-workspace-v1`** (waybar workspace pills): On bind, creates one `ExtWorkspaceGroupHandleV1` associated to the output; creates one `ExtWorkspaceHandleV1` per grid cell (col, row); advertises id, name, coordinates (packed u32), state (`State::Active` for focused, `State::empty()` for others), and capabilities (`WorkspaceCapabilities::Activate`). Clients can activate workspaces; compositor responds with workspace switch + IPC event + state broadcast. On any navigation, `ext_workspace_update_all()` pushes new active state to all handles and `done` to all managers.

2. **`wlr-foreign-toplevel-management-unstable-v1`** (taskbar window list): On bind, snapshots all existing windows and creates `ZwlrForeignToplevelHandleV1` per window with title/app_id/state/done. New windows → `wlr_new_toplevel()`; closed windows → `wlr_remove_toplevel()`. State encoding: activated=4, fullscreen=8, packed as little-endian u32 bytes. Handles: `Activate` (workspace switch + keyboard focus), `Close` (send_close), `SetFullscreen`/`UnsetFullscreen` (toggle via `toggle_fullscreen()`), `Destroy` (cleanup). Focus and state changes broadcast to all handles via `wlr_update_all_states()`.

#### Smithay notes

| Issue | Resolution |
|-------|-----------|
| `delegate_dispatch!` macros conflict with direct `impl Dispatch for GlobalState` | Removed all delegation macros; direct impls don't need them |
| `h.state(u32)` type mismatch | ext-workspace expects `ext_workspace_handle_v1::State` bitflags, not raw u32 |
| `group.workspace()` does not exist | Event is `workspace_enter()` (per protocol XML); `workspace_leave()` exists too |
| `for wl_out in &outputs` — outputs is a Vec<WlOutput> | `output_enter()` takes `&WlOutput`; iterate by value, pass by reference |
| `drop(window)` on shared ref does nothing | Use `let _ = window;` to end the borrow scope |
| `toggle_fullscreen(coords)` wrong arity | Actual signature is `fn toggle_fullscreen(&mut self)` — no args |

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 15 — 2026-05-22

### What was built

**Phase 6 continued: additional protocols + diagonal workspace nav + cursor env + XDG autostart**

#### Files updated

```
griddy/src/handlers/protocols.rs    Added viewporter, single_pixel_buffer_v1, fractional_scale_v1,
                                      content_type_v1, wlr_data_control_unstable_v1 delegates + impls
griddy/src/state.rs                  Added 5 new state fields (viewporter, single_pixel_buffer,
                                       fractional_scale, content_type, data_control);
                                       set XCURSOR_THEME/XCURSOR_SIZE env vars in new() + on reload;
                                       run_xdg_autostart() reads ~/.config/autostart/*.desktop;
                                       spawn_cmd() helper, dirs_xdg_autostart() helper
griddy/src/grid/mod.rs               Added wrap_x()/wrap_y() accessors
griddy/src/keybind/dispatcher.rs     Added WorkspaceNW/NE/SW/SE Action variants;
                                       nav_workspace_diagonal() helper; apply_delta() helper;
                                       parse_action entries for workspace-nw/ne/sw/se
griddy/src/keybind/mod.rs            Default binds: $mod+Ctrl+y/u/b/n → workspace-nw/ne/sw/se
```

#### Key behaviors implemented

1. **`viewporter`** (wp_viewporter): Clients can set source/destination crop/scale on surfaces. Required by many compositing clients, video players (mpv), and anything using fractional scaling.

2. **`single-pixel-buffer-v1`**: Clients can allocate a 1×1 solid-color buffer via `wp_single_pixel_buffer_manager_v1`. Used by layer-shell background surfaces, cursor themes, and solid-color window decorations.

3. **`fractional-scale-v1`**: Clients receive fractional scale hints (`wp_fractional_scale_v1`), used with HiDPI monitors that aren't integer-scaled (e.g. 1.5×). Required for crisp rendering on non-standard DPI.

4. **`content-type-v1`**: Clients can advertise their content type (`video`, `photo`, `game`, `none`). The compositor reads this to apply policies (e.g. allow tearing for games, disable blur for video).

5. **`wlr-data-control-unstable-v1`**: Clipboard managers (cliphist, wl-clipboard, `wl-paste --watch`) can read and write the clipboard/primary-selection via `zwlr_data_control_manager_v1`. Required for clipboard history tools.

6. **Cursor theme env vars**: `XCURSOR_THEME` and `XCURSOR_SIZE` are set in `GlobalState::new()` from `theme.toml [cursor]` settings and re-applied on every config reload. Child processes (apps) inherit the correct cursor theme.

7. **XDG autostart** (`honor_xdg_autostart = true` default): On startup, reads `~/.config/autostart/*.desktop`, filters to `Type=Application`, respects `Hidden=true`, `OnlyShowIn`, and `NotShowIn` (checks for `GriddyWM`), strips `%f/%u/…` field codes from Exec, and spawns the entries. Tools like `nm-applet`, `dunst`, `polkit-agent`, and `cliphist` configured here will auto-start.

8. **Diagonal workspace navigation** (§5, §9.1): Four new dispatchers `workspace-nw`, `workspace-ne`, `workspace-sw`, `workspace-se` navigate diagonally. Both axes move simultaneously with independent per-axis wrap. If either axis can't move (edge + no wrap), the whole move is silently skipped. Default binds: `$mod+Ctrl+y/u/b/n`. The slide animation uses the diagonal direction vector via `slide_direction()`.

#### Smithay notes

| Issue | Resolution |
|-------|-----------|
| `delegate_fractional_scale!` requires `FractionalScaleHandler` | Add empty `impl FractionalScaleHandler for GlobalState {}` (all methods have defaults) |

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

---

## Session 16 — 2026-05-22

### What was built

**Phase 6 continued: Hardware keybinds, overview improvements, hot-corner, locked/global bind flags**

#### Files updated

```
griddy/src/config/types.rs      Added OverviewConfig { hot_corner, hot_corner_delay_ms,
                                  show_titles, show_labels, scale, gap_px, margin_px };
                                  added overview: OverviewConfig to root Config
griddy/src/keybind/mod.rs       Added locked/global fields to BindEntry; from_config() populates
                                  them; lookup_flags() returns (Action, locked, global);
                                  added bl() helper for locked binds, bg() for global binds;
                                  hardware keybind defaults: XF86Audio*, XF86MonBrightness*,
                                  Print, and $mod+Shift+Escape/$mod+Shift+e as global binds
griddy/src/keybind/dispatcher.rs  Added OverviewWindowNext, OverviewWindowPrev, OverviewGrabWindow,
                                    OverviewDropWindow, OverviewCancelGrab, Screenshot actions +
                                    parse entries; overview_move_focus() helper moves grabbed
                                    window to target workspace when arrow keys are pressed;
                                    OverviewActivate respects Tab-selected window and drops
                                    grabbed windows; OverviewToggle resets grab state on exit;
                                    OverviewFocusLeft/Right/Up/Down route through overview_move_focus
griddy/src/state.rs              Added hot_corner_entered: Option<Instant>,
                                  is_locked: bool, overview_window_idx: usize,
                                  overview_grabbed_window: Option<WindowId>,
                                  overview_grab_origin: Option<(u8,u8)>
griddy/src/backend/winit.rs     Added XKB_KEY_TAB + XKB_KEY_SPACE constants;
                                  Tab/Shift+Tab → OverviewWindowNext/Prev intercept;
                                  Space → OverviewGrabWindow/DropWindow intercept;
                                  Escape → OverviewCancelGrab when grab active;
                                  Hot-corner dwell logic in pointer motion handler;
                                  lookup_flags() used in keybind dispatch (locked-aware);
                                  is_in_hot_corner() helper function
```

#### New file

```
dist/griddy.desktop             Wayland session file for LightDM/SDDM/GDM/greetd (§14)
```

#### Key behaviors implemented

1. **Hardware keybind defaults** (§9.1): XF86AudioRaiseVolume/LowerVolume/Mute/MicMute, XF86MonBrightnessUp/Down, XF86AudioPlay/Next/Prev, Print — all `locked = true` so they fire through screen lock.

2. **`locked`/`global` bind flags** (§9): `BindEntry` now stores both flags. `lookup_flags()` returns the full tuple. Keyboard dispatch checks `is_locked` and skips non-locked binds. `bg()` helper creates global binds; `$mod+Shift+Escape` and `$mod+Shift+e` are now global.

3. **`OverviewConfig`** (§7.2, §8): `[overview]` block in `config.toml` with `hot_corner`, `hot_corner_delay_ms`, `show_titles`, `show_labels`, `scale`, `gap_px`, `margin_px`. Defaults: `hot_corner = "top-left"`, `hot_corner_delay_ms = 150`.

4. **Hot-corner entry** (§7.2): Cursor dwelling in the configured corner for `hot_corner_delay_ms` fires `OverviewToggle`. Only active when not already in overview and no drag is active.

5. **Tab/Shift+Tab window navigation** (§7.2): In overview mode, Tab cycles `overview_window_idx` through visible windows in the focused thumbnail. Shift+Tab reverses. Enter activates the selected window and exits overview.

6. **Space grab-and-move** (§7.2): Space picks up the Tab-selected window. Arrow keys then move it to adjacent workspace thumbnails (and move the window there via `grid.move_window_to()`). Enter drops it; Escape returns it to its origin workspace.

7. **Screenshot dispatcher**: `screenshot region` / `screenshot screen` run `grim`+`slurp`.

8. **`.desktop` session file** (§14): `dist/griddy.desktop` — ready for `sudo install -Dm644 dist/griddy.desktop /usr/share/wayland-sessions/griddy.desktop`.

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 17 — 2026-05-22

### What was built

**Phase 6: `ext-session-lock-v1` + XWayland (basic)**
**Phase 7 (early): Workspace templates §8.10 + `--replace` / PID file**

#### Files created

```
griddy/src/config/templates.rs      WorkspaceTemplatesConfig, TemplateConfig, TemplateWindowConfig;
                                      loaded from workspace-templates.toml in config dir
griddy/src/handlers/session_lock.rs  SessionLockHandler impl: lock/unlock/new_surface;
                                       delegate_session_lock!
griddy/src/handlers/xwayland.rs     XwmHandler (full trait) + XWaylandShellHandler;
                                      delegate_xwayland_shell!
```

#### Files updated

```
griddy/src/config/mod.rs            Added pub mod templates; load_templates(config_dir) helper
griddy/src/handlers/mod.rs          Added pub mod session_lock; pub mod xwayland
griddy/src/state.rs                 Added SessionLockManagerState, lock_surfaces: Vec<LockSurface>,
                                      XWaylandShellState, x11_wm: Option<X11Wm>,
                                      xwayland_surfaces: Vec<X11Surface>,
                                      xwayland_client: Option<wayland_server::Client>,
                                      templates: WorkspaceTemplatesConfig,
                                      applied_templates: HashSet<(u8, u8)>,
                                      is_locked: bool
griddy/src/backend/mod.rs           run_backend() now takes no_xwayland: bool
griddy/src/backend/winit.rs         XWayland::spawn + calloop event source registration;
                                      XWaylandEvent::Ready → X11Wm::start_wm + DISPLAY env;
                                      X11 surface render elements after tiled windows;
                                      lock surface rendering (exclusive when is_locked);
                                      pub fn run() takes no_xwayland: bool
griddy/src/main.rs                  pid_file_path(), write_pid_file(), cleanup_pid_file(),
                                      replace_running_instance() via nix::kill SIGTERM/SIGKILL;
                                      --replace flag, improved --check (validates templates),
                                      PID file written before run_backend, cleaned up on exit;
                                      passes no_xwayland to run_backend
griddy/src/keybind/dispatcher.rs    Action::ApplyTemplate(u8, u8); apply_workspace_template()
                                      helper; glob_matches(); parse_action "apply-template" /
                                      "workspace-apply-template"; template applied on every
                                      workspace nav (first visit only unless force=true)
```

#### Key behaviors implemented

1. **`ext-session-lock-v1`**: `SessionLockManagerState::new::<GlobalState, _>(&dh, |_| true)` in state. `lock()` sets `is_locked = true`, sends configure (unsigned Size) to existing lock surfaces, calls `confirmation.lock()`. `new_surface()` configures to full output size. Render loop: when `is_locked && !lock_surfaces.is_empty()`, clears to black and renders only lock surfaces — all normal window/layer rendering skipped. Unlocks clear `is_locked` and empty `lock_surfaces`.

2. **XWayland (basic)**: `XWayland::spawn(&dh, None, empty(), true, Stdio::null(), Stdio::null(), |_| {})` → `(XWayland, Client)`. XWayland inserted as a calloop event source. On `XWaylandEvent::Ready { x11_socket, display_number, .. }`: `X11Wm::start_wm(lh.clone(), x11_socket, client)` and `DISPLAY=:<number>` set. X11 surfaces rendered after tiled windows, before Top layer. `xwayland_client` stored in state, consumed via `.take()` inside the callback (not Clone). On `XWaylandEvent::Error`: clears `x11_wm` and `xwayland_surfaces`. `--no-xwayland` flag skips XWayland spawn.

3. **Workspace templates (§8.10)**: `workspace-templates.toml` with `[[template]]` entries (cell=[col,row], windows with slot+exec+app_id). Loaded on startup from config dir. On first navigation to a workspace, `apply_workspace_template()` is called: matches existing windows by glob app_id, places them in the template slot; unmatched windows spawn their `exec` command with a `PlacementHints` stored in `state.spawn_hint`. `applied_templates: HashSet<(u8, u8)>` tracks which workspaces have been initialized. `Action::ApplyTemplate(u8::MAX, u8::MAX)` applies to the current focused workspace.

4. **`--replace` + PID file**: PID written to `$XDG_RUNTIME_DIR/griddy.lock` before `run_backend`; cleaned up on exit. `--replace` calls `replace_running_instance()` which reads the PID file, sends SIGTERM, waits up to 3s (30×100ms), then SIGKILL if still running.

#### Smithay notes

| Issue | Resolution |
|-------|-----------|
| `LockSurfaceConfigure` is private in smithay 0.7.0 | Removed `ack_configure` from impl — trait default is no-op |
| `SelectionTarget` private in `xwm` module | Import from `smithay::wayland::selection::SelectionTarget` |
| `XWaylandEvent::Ready` wrong fields | Correct pattern: `Ready { x11_socket, display_number, .. }` (not `connection`/`display`) |
| `window` moved before logging in `map_window_request` | Log title/class before `set_mapped()`/`push()` calls |
| `LockSurfaceState::size` is `Option<Size<u32, Logical>>` | Pass unsigned width/height (u32 cast from grid.output_w/h) |

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 18 — 2026-05-22

### What was built

**`ext-idle-notify-v1`, workspace forward/back history fix, special workspaces (scratchpad §22.3), mouse warp (§22.5)**

#### Files updated

```
griddy/src/grid/mod.rs          Added forward_history: VecDeque<(u8,u8)> to Grid struct;
                                  init to empty in Grid::new();
                                  switch_workspace() now clears forward_history on manual nav;
                                  workspace_back() fixed: pop from history → push to forward_history
                                    (was circular — old impl pushed current back to front);
                                  workspace_forward() added: pop from forward_history → push to history;
                                  remove_window() guards against (MAX,MAX) sentinel (special ws panic fix);
                                  detach_window_to_special() moves a window to (MAX,MAX) workspace;
                                  attach_window_from_special() returns window from special to a grid ws

griddy/src/state.rs             Added idle_notifier_state: Option<IdleNotifierState<GlobalState>>;
                                  special_workspaces: HashMap<String, Vec<WindowId>>;
                                  active_special: Option<String>;
                                  all initialized (None / empty) in GlobalState::new()

griddy/src/handlers/protocols.rs  Added IdleNotifierHandler impl + delegate_idle_notify!(GlobalState);
                                    IdleInhibitHandler::inhibit/uninhibit now call
                                    notifier.set_is_inhibited() when idle_notifier_state is Some;
                                    added delegate_idle_notify import

griddy/src/backend/winit.rs     IdleNotifierState::new(&display.handle(), lh) initialized in run()
                                  after XWayland setup (uses event_loop.handle());
                                  handle_input() calls notifier.notify_activity(&seat) on every
                                  input event (after reset_idle)

griddy/src/keybind/dispatcher.rs  Action enum: added ToggleSpecial(String), MoveToSpecial(String);
                                    parse_action: "toggle-special", "move-to-special" parsers;
                                    WorkspaceForward dispatch: now fully implemented via
                                      grid.workspace_forward() + trigger_slide + sync_focus;
                                    ToggleSpecial dispatch: toggles active_special, if hiding
                                      returns windows back into focused ws via attach_window_from_special;
                                    MoveToSpecial dispatch: calls grid.detach_window_to_special +
                                      adds to special_workspaces map;
                                    WorkspaceBack/Forward/To/Index dispatches: warp cursor if
                                      warp_cursor_on_workspace_change is set;
                                    warp_cursor_to_focused() public helper;
                                    sync_focus() now warps if warp_cursor_on_focus_change is set;
                                    nav_workspace() warps if warp_cursor_on_workspace_change;
                                    SERIAL_COUNTER added to imports

griddy/src/config/types.rs      InputConfig: added warp_cursor_on_focus_change: bool (default false);
                                  warp_cursor_on_workspace_change: bool (default false)
```

#### Key behaviors implemented

1. **`ext-idle-notify-v1`** (swayidle/hypridle protocol): `IdleNotifierState` added to compositor state. Initialized with the event loop handle in `run()`. Every input event calls `notify_activity(&seat)` — this resets all registered idle timeouts and fires `resumed` events to idle clients. Idle inhibit changes (`idle-inhibit-unstable-v1` `inhibit`/`uninhibit`) now propagate to `set_is_inhibited()` on the notifier state. swayidle and hypridle can now register idle callbacks that fire on timeout and reset on input.

2. **Workspace forward/back history fix**: The `workspace_back()` had a circular bug — it pushed the current workspace back to the front of history, preventing traversal beyond 2 entries. Fixed to use a proper `forward_history: VecDeque<(u8,u8)>` stack. `workspace_back()` pops from `history` and pushes to `forward_history`. `workspace_forward()` does the reverse. Manual `switch_workspace()` clears `forward_history` (same as browser navigation). History depth capped at 64 entries.

3. **Special workspaces / scratchpad (§22.3)**: Windows can be sent to named special workspaces with `move-to-special <name>`. Their `workspace` field is set to the sentinel `(u8::MAX, u8::MAX)`, detaching them from all grid slots. `toggle-special <name>` shows/hides the special workspace. `remove_window()` guards against panicking on the (MAX,MAX) sentinel. When toggling a special workspace on, its windows are attached to the current workspace via `attach_window_from_special()`. Default name is `scratchpad`.

4. **Mouse warping (§22.5)**: Two new config fields under `[input]`: `warp_cursor_on_focus_change` (default false) — warps cursor to focused window center on any focus change; `warp_cursor_on_workspace_change` (default false) — warps on workspace navigation. `warp_cursor_to_focused()` computes the window center via `grid.compute_rect()` and calls `pointer.motion()` to reposition the cursor.

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Where we left off

**Last completed:** ext-idle-notify-v1, workspace forward history fix, special workspaces, mouse warp (Session 18, 2026-05-22)

### Working keybinds

| Keys | Action |
|---|---|
| `Super+Ctrl+hjkl` | Navigate workspace grid (orthogonal) |
| `Super+Ctrl+y/u/b/n` | Navigate workspace grid diagonally (NW/NE/SW/SE) |
| `Super+Ctrl+o / i` | Workspace back / forward in history (proper bidirectional stack) |
| `Super+hjkl` | Spatial focus (crosses workspace boundaries) |
| `Super+f / F / v` | Toggle Fullscreen / TotalFullscreen / Floating |
| `Super+Left / Right` | Assign HalfLeft / HalfRight slot |
| `Super+c` | Center floating window |
| `Super+w` → h/l/u/i/j/k/f/F/v | Placement submap (auto-exits after selection) |
| `Super+r` → h/l/j/k | Resize submap for floating windows (repeat; Escape exits) |
| `Super+Tab` | Cycle slot stack forward (stack-next) |
| `Super+Shift+Tab` | Cycle slot stack backward (stack-prev) |
| `Super+Alt+Tab` | Promote top of stack to current slot (stack-promote) |
| `Super+Alt+Shift+Tab` | Collapse current window back into stack (stack-collapse) |
| `Super+Alt+Up / Down` | Move focused window up/down in its stack |
| `Super+m` | Toggle minimap HUD |
| `Super+1–9` | Jump to workspace by index |
| `Super+Shift+1–9` | Move focused window to workspace by index |
| `Super+Shift+Ctrl+hjkl` | Move focused window one workspace in direction |
| `Super+o` | Toggle overview mode |
| `Super+Shift+Escape` | Exit TotalFullscreen (global — fires inside TotalFullscreen grab) |
| `Super+q` | Close focused window |
| `Super+Shift+e` | Quit compositor (global) |
| `Super+Return` | Spawn kitty |
| `Super+d` | Spawn fuzzel |
| `Super+Shift+r` | Force config reload |
| `XF86AudioRaiseVolume / LowerVolume` | Volume +5% / −5% via wpctl (locked) |
| `XF86AudioMute / MicMute` | Toggle sink / source mute via wpctl (locked) |
| `XF86MonBrightnessUp / Down` | Brightness +5% / −5% via brightnessctl (locked) |
| `XF86AudioPlay / Next / Prev` | Media play-pause / next / prev via playerctl (locked) |
| `Print` | Screenshot region via grim + slurp (locked) |
| *(in overview)* `Tab / Shift+Tab` | Cycle windows in focused thumbnail |
| *(in overview)* `Space` | Grab / drop selected window for move |
| *(in overview)* `Arrow keys` | Navigate thumbnails (moves grabbed window if held) |
| *(in overview)* `Enter / Escape` | Activate workspace / cancel grab |

### Working config files

| File | What it does |
|---|---|
| `~/.config/griddy/config.toml` | Main config: `[grid]`, `[input]`, `[windows]`, `[startup]`, `[env]`, `[[bind]]`, `[[submap]]`, `[overview]` |
| `theme.toml` (same dir) | Gaps, colors, cursor, blur — auto-loaded, live-applied on reload |
| `rules.toml` (same dir) | `[[rule]]` entries with glob app_id/title matching; slot/state/workspace overrides |
| `keybinds.toml` (same dir) | Extra `[[bind]]` entries appended to the main bind table |

`[overview]` block in `config.toml`: `hot_corner` ("top-left"/"top-right"/"bottom-left"/"bottom-right"/"none"), `hot_corner_delay_ms` (default 150), `show_titles`, `show_labels`, `scale`, `gap_px` (default 16), `margin_px` (default 32).

### Implemented

**Window management**
- New-window placement policy (§6.9): first window → Fullscreen; second splits to HalfLeft/HalfRight; next-empty-slot scan after that
- Fullscreen adaptation cascade (§6.5.1): incoming Fullscreen adapts to largest free tiled region
- Slot adaptation (§6.5.1): Half→Quarter when only one of a pair is free
- Conflict resolver: stack / swap / kick-to-floating (§6.5.2)
- Focus-on-close: last-focused → stack-next → slot-neighbor → none (§6.10)
- Spatial focus navigation across workspace boundaries (§6.11)
- Border rendering — focused accent / unfocused idle colors
- $mod + drag: floating window move (BTN_LEFT) / resize (BTN_RIGHT)
- Edge snap: floating → slot at screen edges/corners with live preview (§6.12)
- Unsnap: tiled window drag → floating when > unsnap_threshold_px (§6.12 reverse)

**Animations**
- Workspace slide animations with diagonal support (§7.1)
- Window fade-in animation on map (§11.2 open)
- Slot/position move animation: all slot/state/move actions lerp from old to new rect
- Animation snap-then-start policy for workspace slide interrupts (§11.8)
- `[animations]` config section: open_duration_ms, on_interrupt, crossfade_ms

**Overview & Minimap (§7.2, §7.4)**
- `Super+o` toggles overview: NxN thumbnail tiles with colored window rects
- Arrow keys navigate thumbnails; Enter activates; Escape exits
- Click on thumbnail switches workspace and exits overview
- Tab / Shift+Tab cycles windows within the focused thumbnail
- Space grabs selected window; arrow keys move it to adjacent workspace; Enter drops, Escape cancels
- Hot-corner entry: cursor dwell in configured corner for `hot_corner_delay_ms` fires OverviewToggle
- Minimap HUD in bottom-right corner (focus mode only): 14px cells, accent/idle/bg_alt colors
- Frame callbacks sent to all workspaces during overview (no client stall)
- IPC events: overview_opened, overview_closed, view_mode_changed

**Config**
- Hot-reload: mtime-based, every 500ms + `Super+Shift+r` force reload
- `theme.toml`: gaps, colors, cursor, blur config — live-applied on reload
- `rules.toml`: per-app glob rules for slot/state/workspace/floating overrides
- Submaps (§8.7): placement submap, resize submap; user-defined via config
- app_id / title tracked via XdgToplevelSurfaceRoleAttributes on every commit
- `[overview]` block: hot_corner, hot_corner_delay_ms, show_titles, show_labels, scale, gap_px, margin_px

**Hardware keybinds & bind flags (§9)**
- `locked = true` binds fire even when `is_locked == true` (screen lock): XF86Audio*, XF86MonBrightness*, Print
- `global = true` binds fire even during TotalFullscreen input grab: `Super+Shift+Escape`, `Super+Shift+e`
- `lookup_flags()` returns `(Action, locked, global)`; keyboard dispatch checks `is_locked` before firing
- Screenshot dispatcher: `screenshot region` → `grim -g "$(slurp)"`, `screenshot screen` → `grim`

**Wayland session**
- `dist/griddy.desktop` — install to `/usr/share/wayland-sessions/` for LightDM/SDDM/GDM/greetd

**IPC (§12)**
- Dual-socket: command socket + event socket
- griddyctl CLI: dispatch, get (windows/workspaces/activewindow/grid/monitors/version), reload, kill, batch, subscribe
- Events: workspace_changed, window_opened, window_closed, window_focus, submap_changed, config_reloaded, config_error, overview_opened, overview_closed, view_mode_changed

**Session lock**
- `ext-session-lock-v1`: swaylock/hyprlock/gtklock supported; renders only lock surfaces when locked; hardware keybinds (`locked = true`) still fire through lock
- `is_locked` state gating: non-locked binds suppressed during lock; all normal window rendering skipped

**XWayland**
- XWayland spawned via `XWayland::spawn`; `X11Wm` started on `XWaylandEvent::Ready`
- X11 surfaces rendered after tiled windows, before Top layer shell
- `DISPLAY` env var set for child X11 app launches
- Override-redirect windows (menus, tooltips) tracked and rendered
- `--no-xwayland` flag disables XWayland for a session

**Workspace templates (§8.10)**
- `workspace-templates.toml`: `[[template]]` entries with cell=[col,row] and `[[template.window]]` slots
- Applied on first visit to each workspace; `apply-template` / `workspace-apply-template` dispatcher forces re-apply
- Existing windows matched by glob app_id and placed in template slot; unmatched windows spawned with exec + placement hints
- `applied_templates: HashSet<(u8, u8)>` tracks initialized workspaces

**Compositor lifecycle**
- `griddy --replace`: sends SIGTERM to running instance, waits 3s, SIGKILL if needed; PID file in `$XDG_RUNTIME_DIR/griddy.lock`
- `griddy --check`: validates config + templates, exits 0/1
- `griddy --no-xwayland`: disables XWayland for the session

**Shell / Protocol**
- `wlr-layer-shell` v4: waybar, mako, dunst, swaybg, fuzzel etc. fully supported
- Exclusive zone handling: layer bars reserve screen area; tiled windows avoid them
- Z-order: Background → Bottom → tiled windows → Fullscreen → Overlay → TotalFullscreen → Floating → cursor
- `wl_output` v4 + `xdg_output_unstable_v1`: output geometry advertised to all clients
- `xdg-decoration-unstable-v1`: always ServerSide; suppresses client-side chrome
- `ext-foreign-toplevel-list-v1`: live window list for taskbars/switchers
- `cursor-shape-v1`: clients request cursor shapes (pointer, text, resize, etc.)
- `presentation-time` (wp_presentation): per-frame feedback for smooth client pacing
- `idle-inhibit-unstable-v1`: mpv, games, fullscreen video inhibit idle (propagates inhibit state to ext-idle-notify)
- `ext-idle-notify-v1`: swayidle/hypridle register idle callbacks; fired on timeout, reset on any input; inhibited by idle-inhibit clients
- `xdg-activation-v1`: activation tokens; focus + workspace switch on request
- `relative-pointer-v1`: delta motion events for games and VM display clients
- `pointer-constraints-v1`: lock/confine constraints registered; activated on focus
- Pointer event forwarding: motion, button, axis, relative motion all reach Wayland clients
- `ext-workspace-v1`: workspace grid exposed to waybar/eww; activate requests switch workspace
- `wlr-foreign-toplevel-management-unstable-v1`: live window list for taskbars; activate/close/fullscreen supported
- `viewporter`: wp_viewporter crop/scale for clients
- `single-pixel-buffer-v1`: solid-color 1×1 buffers
- `fractional-scale-v1`: HiDPI fractional scale hints
- `content-type-v1`: per-surface content type hints
- `wlr-data-control-unstable-v1`: clipboard managers supported
- Cursor theme + size env vars: XCURSOR_THEME/XCURSOR_SIZE set from theme.toml on startup + reload
- XDG autostart: ~/.config/autostart/*.desktop entries spawned on startup
- Diagonal workspace nav: workspace-nw/ne/sw/se dispatchers + $mod+Ctrl+y/u/b/n default binds

### Not yet implemented

- DRM/KMS backend — only `winit` dev backend available; OS-level pointer lock/DPMS not possible
- Window close fade-out (requires per-window FBO / texture capture)
- Shadows, rounded corners, blur pipeline
- Per-event GLSL shader hooks (§11.2–§11.5)
- OSD system §8.9 (submap/reload indicator — needs font renderer)
- `xdg-desktop-portal` backend
- Live window thumbnails in overview (needs per-window FBOs)
- Workspace labels in overview / OSD (needs font renderer)
- Drag-and-drop windows between overview thumbnails (needs overview pointer tracking)
- 4-finger swipe gestures (needs libinput gesture support)
- `window_slot_adapted` / `window_state_adapted` emission (conflict resolver needs to return adaptation info)
- `monitor_added` / `monitor_removed` emission (needs DRM hot-plug events)
- `keyboard_layout_changed` emission (needs keyboard layout switch detection)
- `notification_daemon_missing` emission (needs D-Bus availability check)

---

## What comes next

### Phase 4 — Renderer & Shaders (remaining)

| Item | Spec | Status |
|---|---|---|
| Window open fade-in | §11.2 | ✅ done |
| Slot move animation | §11.2 move | ✅ done |
| Animation snap-then-start | §11.8 | ✅ done |
| `wlr-layer-shell` | §6 | ✅ done |
| `wl_output` + `xdg_output` | §6 | ✅ done |
| `xdg-decoration` | §6 | ✅ done |
| `ext-foreign-toplevel-list` | §6 | ✅ done |
| Window close fade-out | §11.2 close | later (needs per-window FBO) |
| DRM/KMS backend | §4 | later |
| Rounded corners | §8.5 | later |
| Drop shadows | §8.5 | later |
| Blur pipeline | §8.5 | later |
| Per-event GLSL shader hooks | §11.2–§11.5 | later |

### Phase 5 — Overview & Navigation

| Item | Status |
|---|---|
| Zoomed-out grid view with thumbnail tiles | ✅ done |
| Keyboard navigation (arrows/Enter/Escape) | ✅ done |
| Click-to-activate workspace | ✅ done |
| IPC events (overview_opened, overview_closed) | ✅ done |
| Minimap HUD (§7.4) | ✅ done |
| Tab/Shift+Tab window cycle in thumbnail | ✅ done |
| Space grab-and-move window in overview | ✅ done |
| Hot-corner entry | ✅ done |
| Drag windows between overview thumbnails | later (needs overview pointer tracking) |
| 4-finger swipe | later (needs libinput gesture support) |
| Live window thumbnails | later (needs per-window FBOs) |
| Workspace labels in overview | later (needs font renderer) |

### Phase 6 — Shell / Protocol

| Item | Status |
|---|---|
| `wlr-layer-shell` v4 | ✅ done |
| `xdg-decoration-unstable-v1` | ✅ done |
| `ext-foreign-toplevel-list-v1` | ✅ done |
| `cursor-shape-v1` | ✅ done |
| `presentation-time` | ✅ done |
| `idle-inhibit-unstable-v1` | ✅ done |
| `ext-idle-notify-v1` (swayidle/hypridle) | ✅ done |
| `xdg-activation-v1` | ✅ done |
| `relative-pointer-v1` | ✅ done |
| `pointer-constraints-v1` | ✅ done (soft — winit can't lock OS cursor) |
| Pointer event forwarding (motion/button/axis) | ✅ done |
| `ext-workspace-v1` (waybar workspace pills) | ✅ done |
| `wlr-foreign-toplevel-management-unstable-v1` | ✅ done |
| `viewporter` + `single_pixel_buffer_v1` | ✅ done |
| `fractional_scale_v1` + `content_type_v1` | ✅ done |
| `wlr-data-control-unstable-v1` (clipboard managers) | ✅ done |
| Cursor theme env vars + XDG autostart | ✅ done |
| Diagonal workspace nav (`workspace-nw/ne/sw/se`) | ✅ done |
| Hardware keybinds (XF86Audio*, brightness, screenshot) | ✅ done |
| `locked`/`global` bind flags | ✅ done |
| OverviewConfig (`[overview]` block in config) | ✅ done |
| Hot-corner entry for overview | ✅ done |
| `.desktop` session file (§14) | ✅ done |
| OSD system §8.9 (submap/reload indicator) | later (needs font renderer) |
| Session lock (`ext-session-lock-v1`) | ✅ done |
| `xdg-desktop-portal` backend | later |
| XWayland | ✅ done (basic — spawn + X11Wm + surface render) |

### Phase 7 — Polish, Templates, Plugin ABI, 1.0

| Item | Status |
|---|---|
| `workspace-templates.toml` (§8.10) | ✅ done (basic — load, apply on first visit, glob match, exec spawn) |
| `griddy --replace` live upgrade path | ✅ done (SIGTERM + SIGKILL via nix, PID file) |
| Workspace back/forward history (§5) | ✅ done (bidirectional with forward_history stack) |
| Special workspaces / scratchpad (§22.3) | ✅ done (toggle-special, move-to-special, (MAX,MAX) sentinel) |
| Mouse cursor warp (§22.5) | ✅ done (warp_cursor_on_focus_change, warp_cursor_on_workspace_change) |
| Focus stealing prevention (§22.5) | ✅ done (blocks new-window focus steal; marks window urgent) |
| Window urgency (§22.6) | ✅ done (`is_urgent` on Window; warn border color; cleared on focus) |
| IPC events: `lock`, `unlock`, `urgent` (§12.2) | ✅ done (emitted from session-lock handler and xdg_shell) |
| `reload-theme` action (§8.5) | ✅ done (reloads theme.toml only, emits ConfigReloaded{which:"theme"}) |
| `cursorpos` IPC query returns real position (§12.1) | ✅ done (was hardcoded 0,0; now uses state.cursor_pos) |
| Action stubs: `osd-show`, `cycle-shader`, `cheatsheet-toggle` | ✅ done (parsed, log-only) |
| `is_urgent` in IPC window output (§12.1) | ✅ done (JSON + plaintext) |
| `warn` color in theme (§4) | ✅ done (default #e0af68; used for urgent borders) |
| IPC event name fix: `idle_timeout`→`idle`, `idle_resume`→`resume` (§12.2) | ✅ done |
| `window_title` / `window_app_id` events on commit (§12.2) | ✅ done (change-detected in compositor.rs) |
| Theme color tokens: `danger`, `ok`, `shadow` (§8.5) | ✅ done (defaults: #f7768e, #9ece6a, #000000aa) |
| `stack-flip <n>` action (§6.8) | ✅ done (jump to stack index n) |
| IPC command stubs: `keyword`, `layers`, `animations`, `shaders`, `getoption`, `setprop`, `notify`, `globalshortcuts` (§12.1) | ✅ done |
| `move-window <col,row>` action alias (§9.2) | ✅ done |
| IPC event catalogue completion (§12.2) | ✅ done (24 events total; all spec events represented) |
| TotalFullscreen workspace protection for `move-window-direction` (§6.7.1) | ✅ done (skips TF workspaces, emits window_move_skipped/window_move_refused) |
| `window_placed` event (§6.9) | ✅ done (emitted on new_toplevel after window_opened) |
| `window_state_changed` / `window_slot_changed` event emission | ✅ done (all toggle/assign actions) |
| `window_stack_changed` event emission (§6.8) | ✅ done (all stack operations) |
| `window_moved` event emission (§6.7) | ✅ done (move-window-to/index dispatchers) |
| `workspace_sync_changed` event name fix (§5) | ✅ done (was workspace_sync_toggled) |
| `theme_reloaded` event (§8.5) | ✅ done (emitted alongside config_reloaded on reload-theme) |
| `safe_mode_entered` event (§22.2) | ✅ done (pushed to pending_events at startup) |
| Plugin C ABI (`cdylib`) with stable versioned interface | later |
| Packaged default config, themes, and man pages | later |

---

## Session 19 — 2026-05-22

### What was built

**§22.5 Focus stealing prevention + §22.6 Window urgency**

- `griddy/src/grid/window.rs`: Added `is_urgent: bool` field to `Window` struct.
- `griddy/src/grid/mod.rs`: `add_window()` initializes `is_urgent: false`; `set_focus()` clears `is_urgent` and guards against `(MAX,MAX)` sentinel.
- `griddy/src/handlers/xdg_shell.rs`: `new_toplevel()` checks `focus_steal_prevention`; if a background window would steal focus, reverts `focused_window` to previous value and sets `is_urgent = true`, emits `Event::Urgent`.
- `griddy/src/keybind/dispatcher.rs`: `sync_focus()` clears `is_urgent` on the newly-focused window (covers keyboard navigation path).

**IPC events: `lock` / `unlock` / `urgent` (§12.2)**

- `griddy/src/ipc/events.rs`: Added `Urgent { id }`, `SessionLocked`, `SessionUnlocked` variants and their `format()` cases.
- `griddy/src/handlers/session_lock.rs`: Emits `Event::SessionLocked` and `Event::SessionUnlocked` from `lock()` and `unlock()`.

**Urgent border rendering**

- `griddy/src/config/theme.rs`: Added `warn: String` to `ColorConfig` (default `#e0af68`).
- `griddy/src/backend/winit.rs`: Added `is_urgent: bool` to `RenderItem` and `WindowRenderData`; parsed `warn` color; border color now uses `warn` for urgent unfocused windows.

**`reload-theme` action**

- `griddy/src/config/mod.rs`: Made `load_theme_file` public.
- `griddy/src/keybind/dispatcher.rs`: Added `Action::ReloadTheme`; parsed as `"reload-theme"`; dispatched via `state.reload_theme()`.
- `griddy/src/state.rs`: Added `GlobalState::reload_theme()` — reloads only `theme.toml` from config dir, emits `ConfigReloaded { which: "theme" }`.

**`cursorpos` IPC fix**

- `griddy/src/ipc/commands.rs`: `cursorpos` now returns `state.cursor_pos` cast to `i64` instead of hardcoded `0,0`.

**Action stubs**

- `griddy/src/keybind/dispatcher.rs`: Added `Action::OsdShow(String)`, `Action::CycleShader`, `Action::CheatsheetToggle` — parsed from `"osd-show"`, `"cycle-shader"`, `"cheatsheet-toggle"`; dispatch logs and returns false.

**`is_urgent` in IPC window output**

- `griddy/src/ipc/commands.rs`: `window_to_str()` now includes `"is_urgent": bool` in JSON and appends ` urgent` in plaintext when set.

### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 20 — 2026-05-22

### What was built

**IPC event name fixes (§12.2)**

- `griddy/src/ipc/events.rs`: `IdleTimeout` now formats as `idle` (was `idle_timeout`); `IdleResume` now formats as `resume` (was `idle_resume`) — matches spec event catalogue exactly.

**`window_title` / `window_app_id` events (§12.2)**

- `griddy/src/handlers/compositor.rs`: In `commit()`, reads previous `app_id`/`title` values before calling `update_window_metadata()`, then emits `Event::WindowTitle` / `Event::WindowAppId` only when the values actually change. Bars and scripts can now react to dynamic title changes (e.g. terminal `cd` changes).

**Theme color tokens: `danger`, `ok`, `shadow` (§8.5)**

- `griddy/src/config/theme.rs`: Added `danger` (`#f7768e`), `ok` (`#9ece6a`), and `shadow` (`#000000aa`) to `ColorConfig` with serde defaults. Now matches the full spec `[colors]` block.

**`stack-flip <n>` action (§6.8)**

- `griddy/src/grid/mod.rs`: Added `Grid::stack_flip(n)` — rotates the slot stack so index `n` becomes top. No-op if `n` out of bounds.
- `griddy/src/keybind/dispatcher.rs`: Added `Action::StackFlip(u32)`; parsed from `"stack-flip"`; dispatched via `state.grid.stack_flip(n)` + `sync_focus`.

**Missing IPC commands (§12.1)**

- `griddy/src/ipc/commands.rs`:
  - `keyword <key> <value>`: runtime override for `input.follow_mouse`, `input.cross_workspace_focus`, `gaps.windows.inner_px/outer_px`, `animations.open_duration_ms`; unknown keys return `ok` (logged).
  - `layers`: returns empty JSON list `[]`.
  - `animations`: returns `{"open_duration_ms":N}`.
  - `shaders`: returns empty JSON list `[]`.
  - `getoption <key>`: looks up above keys; unknown keys return error.
  - `setprop <id> <prop> <value>`: currently supports `is_urgent`; unknown props return error.
  - `notify <icon> <ms> <msg>`: logs and returns `ok`.
  - `globalshortcuts`: returns empty JSON list `[]`.

**Action alias (§9.2)**

- `griddy/src/keybind/dispatcher.rs`: Added `"move-window"` as alias for `move-window-to <col,row>`.

### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 21 — 2026-05-23

### What was built

**Phase 7: Plugin ABI (§13) + Packaged defaults**

#### Files created

```
griddy/src/plugins/mod.rs       Plugin ABI loader: load_plugin(), load_plugins_file(),
                                  LoadedPlugin with 6 lifecycle hooks,
                                  C-compatible GriddyPluginDescriptor / GriddyWindowInfo,
                                  no-op stubs when feature disabled, GRIDDY_PLUGIN_ABI_VERSION=1
dist/griddy_plugin.h            C header for plugin authors: full ABI surface with docs
                                  and a minimal "hello" example in the header comment
dist/default.toml               Commented default config embedded in binary for first-run
dist/griddy.1                   Man page: NAME, SYNOPSIS, OPTIONS, CONFIGURATION,
                                  IPC, SIGNALS, ENVIRONMENT, FILES, SEE ALSO
dist/contrib/waybar.jsonc       Sample Waybar config with workspace pills + taskbar
dist/contrib/mako.conf          Sample Mako notification daemon config (GriddyWM colors)
dist/contrib/griddyctl-workspace-pills.sh   Workspace pills helper script for Waybar
```

#### Files updated

```
Cargo.toml (workspace)          Added libloading = "0.8" to workspace deps
griddy/Cargo.toml               Added plugin-abi feature (default); optional libloading dep
griddy/src/main.rs              Added `mod plugins`; first_run_copy_default_config();
                                  include_str!("../../dist/default.toml") embedded in binary
griddy/src/ipc/events.rs        Added PluginError { name, message } and PluginLoaded { name }
griddy/src/state.rs             Added plugins: Vec<LoadedPlugin>; loaded in new() from plugins.toml
griddy/src/backend/winit.rs     frame_begin / pre_render / post_render / frame_end plugin hooks
griddy/src/handlers/xdg_shell.rs  window_pre_open + window_post_close plugin hooks
```

#### Key behaviors implemented

1. **Plugin C ABI (§13)**: `cdylib` shared libraries export `griddy_plugin_init()` returning a static descriptor. ABI version checked; mismatch skips plugin + logs warning. All hook fields nullable.
2. **6 lifecycle hooks**: frame_begin, frame_end (frame bracket), pre_render (before GLES open), post_render (after GLES finish), window_pre_open, window_post_close.
3. **`plugin-abi` feature** (default on): no-op stubs when disabled; zero overhead.
4. **`plugins.toml`** in config dir: `[[plugin]]` with path, enabled, config.
5. **First-run config copy**: `~/.config/griddy/config.toml` created from embedded default on first launch (no config file found).
6. **Man page**, **contrib waybar/mako configs**, **workspace pills script**.

#### Build status

```
cargo build --workspace   →   clean (0 errors, 0 warnings)
```

---

## Session 22 — Protocols, Signal Handling, Monitors Config, Unit Tests

### Changes

#### New files

```
dist/griddyctl.1                     Man page for griddyctl CLI
dist/monitors.toml                   Sample monitors.toml with inline docs
griddy/src/config/monitors.rs        monitors.toml config types + loader (§8.6)
```

#### Files updated

```
Cargo.toml (workspace)               calloop gains `signals` feature
griddy/src/config/mod.rs             Added monitors module + re-exports
griddy/src/state.rs                  Added monitors_config: MonitorsConfig field + load at startup
griddy/src/backend/winit.rs
  - Keyboard shortcuts inhibit check (before keybind dispatch)
  - Touch event forwarding: TouchDown / TouchMotion / TouchUp / TouchCancel / TouchFrame
  - SIGUSR1 → force config reload (calloop Signals source)
griddy/src/handlers/protocols.rs     (already complete from session; keyboards-inhibit-state)
griddy/src/grid/layout.rs            +9 unit tests (slot rects, fullscreen, overview, minimap)
griddy/src/ipc/events.rs             +7 unit tests (wire format, newline invariant)
```

#### Key behaviors implemented

1. **Keyboard shortcuts inhibit** (`zwp_keyboard_shortcuts_inhibit_manager_v1`): Games/VM clients that activate an inhibitor bypass compositor keybinds entirely. `data.seat.keyboard_shortcuts_inhibited()` checked before every keybind lookup.
2. **Touch event forwarding** (`wl_touch`): `TouchDown` finds the surface under the touch point via `pointer_focus_for_cursor()`, constructs smithay `DownEvent`/`MotionEvent`/`UpEvent`, dispatches through `seat.get_touch()`. `TouchFrame` sends frame packets.
3. **SIGUSR1 config reload**: calloop `Signals::new(&[Signal::SIGUSR1])` source inserted into the event loop. Fires `reload_config_if_changed(true)` so `kill -SIGUSR1 $PID` is equivalent to `griddyctl reload`.
4. **`monitors.toml`** (§8.6): Full config types (`MonitorConfig`, `MonitorDefaults`, `OutputTransform`, `VrrPolicy`, `MonitorsConfig`). `load_monitors(config_dir)` called at startup; result stored in `GlobalState::monitors_config`. Ready for DRM backend mode-setting.
5. **25 unit tests passing**: layout (9), IPC events (7), rules glob (3), monitors TOML parsing (6).
6. **`griddyctl.1` man page**: Full man page with SYNOPSIS, COMMANDS, OPTIONS, JSON/BATCH docs, ENVIRONMENT, FILES, EXAMPLES.

#### Build status

```
cargo test --workspace   →   25 passed; 0 failed; 0 warnings
```

---

## Session 23 — 2026-05-23

### What was built

**§6.13 Min-size overflow policy, §22.4 Window swallowing, Named theme presets, Session persistence (§22.1), Crash recovery (§22.2), IME protocols (§22.18), security-context-v1 (§16)**

#### New files

```
dist/themes/catppuccin-mocha.toml    Catppuccin Mocha dark theme preset
dist/themes/catppuccin-latte.toml    Catppuccin Latte light theme preset
dist/themes/tokyo-night.toml         Tokyo Night theme preset
dist/themes/tokyo-night-light.toml   Tokyo Night Light theme preset
dist/themes/nord.toml                Nord theme preset
dist/themes/gruvbox-dark.toml        Gruvbox Dark theme preset
dist/themes/gruvbox-light.toml       Gruvbox Light theme preset
dist/themes/dracula.toml             Dracula theme preset
dist/themes/rose-pine.toml           Rosé Pine (Main) theme preset
dist/themes/rose-pine-dawn.toml      Rosé Pine Dawn (light) theme preset
dist/themes/solarized-dark.toml      Solarized Dark theme preset
dist/themes/solarized-light.toml     Solarized Light theme preset
dist/themes/everforest-dark.toml     Everforest Dark (medium) theme preset
griddy/src/session.rs                Session state persistence (§22.1): load/save/clear,
                                       SessionWindow/SessionState, take_hint() restore lookup,
                                       6 unit tests
```

#### Files updated

```
griddy/src/config/mod.rs            load_theme_file() now resolves `import = "preset-name"`:
                                      searches ~/.config/griddy/themes/<name>.toml then
                                      /usr/share/griddy/themes/<name>.toml; loads preset as
                                      base then overlays user's theme on top;
                                      resolve_theme_preset() helper with path-traversal guard
griddy/src/main.rs                  Added `mod session`; crash recovery (§22.2):
                                      record_crash_and_check_safe_mode() writes timestamps to
                                      $XDG_STATE_HOME/griddy/crash_history, returns true if ≥3
                                      crashes in 30s → safe_mode skips user config;
                                      clear_crash_history() on clean exit; save session on clean exit
griddy/src/state.rs                 Added `session: Option<SessionState>` + `safe_mode: bool` fields;
                                      GlobalState::new() gains `safe_mode: bool` param;
                                      loads session state at startup (skipped in safe mode);
                                      registers text-input-v3, input-method-v2, security-context-v1
griddy/src/session.rs               (see above)
griddy/src/backend/winit.rs         Calls crate::session::save(&state) on clean exit
griddy/src/handlers/xdg_shell.rs    new_toplevel: session restore applied before swallow check;
                                      take_hint(&app_id, &title) → workspace + slot hints
griddy/src/handlers/protocols.rs    Added IME: TextInputManagerState (text-input-v3) +
                                      InputMethodManagerState (input-method-v2) + stub
                                      InputMethodHandler impl; SecurityContextHandler stub +
                                      delegate macros; updated doc comment
griddy/src/ipc/commands.rs          Added "session" command: save/restore/clear/status subcommands
griddyctl/src/main.rs               Added `Session { Save, Restore, Clear, Status }` subcommand
                                      → sends "session <sub>" to IPC socket
```

#### Key behaviors implemented

1. **Named theme presets (§8.5)**: 13 preset files in `dist/themes/`. `load_theme_file()` resolves `import = "catppuccin-mocha"` → loads preset from XDG or `/usr/share/griddy/themes/` → merges user's overrides on top. Path-traversal guard rejects names containing `/` or `..`.

2. **§6.13 Min-size overflow** (completed in previous session; formally documented here): When a tiled window's `min_size` exceeds its slot, and policy is `Float`, it is auto-promoted to floating centered at min_size. `WindowSizeConstraintFloat` IPC event emitted.

3. **§22.4 Window swallowing** (completed in previous session): Terminal-launched GUI apps inherit the terminal's slot. Terminal hidden until GUI closes, then restored. Detected via `/proc/<pid>/status` PPid + Wayland client credentials.

4. **Session state persistence (§22.1)**:
   - `save()`: serializes all non-special, non-swallowed windows to `$XDG_STATE_HOME/griddy/session.json` on clean exit.
   - `load()`: reads session.json at startup.
   - `take_hint(app_id, title)`: exact-match (both fields) > app_id-only; each entry consumed once.
   - On `new_toplevel`: session hint applied if no rule override present.
   - `griddyctl session save/restore/clear/status` IPC commands.

5. **Crash recovery / safe mode (§22.2)**:
   - Startup timestamps written to `$XDG_STATE_HOME/griddy/crash_history`.
   - ≥3 timestamps within 30s → `safe_mode = true`: user config skipped (loads defaults only), session not restored.
   - Clean exit clears crash_history.
   - `safe_mode` exposed in `griddyctl session status`.

6. **IME protocols (§22.18)**:
   - `text-input-v3` (`zwp_text_input_manager_v3`): registered via `TextInputManagerState::new`.
   - `input-method-v2` (`zwp_input_method_manager_v2`): registered with stub `InputMethodHandler` (no-op popup management; full popup placement needs font metrics).
   - fcitx5, ibus, virtual-keyboard clients can now connect.

7. **`security-context-v1` (§16)**:
   - `wp_security_context_manager_v1` registered with permissive filter.
   - `context_created` stub logs sandbox engine; drops the listener source (no per-context protocol restrictions yet — reserved for future sandboxing policy).
   - Flatpak/snap clients can now negotiate security contexts.

#### Build status

```
cargo test --workspace   →   31 passed; 0 failed; 0 warnings
```

---

## Session 24 — 2026-05-23

### What was built

**IPC event catalogue completion (§12.2) + TotalFullscreen workspace protection (§6.7.1) + event emission wiring**

#### Files updated

```
griddy/src/ipc/events.rs        Expanded Event enum: added WorkspaceCreated, WorkspaceRenamed
                                  (with old/new fields), WorkspaceSyncChanged (renamed from
                                  WorkspaceSyncToggled, now emits "synced"/"unsynced" string),
                                  WindowPlaced, WindowSlotAdapted, WindowStateAdapted,
                                  WindowMoveSkipped, WindowMoveRefused, WindowStackChanged,
                                  ThemeReloaded, ShaderLoaded, ShaderError,
                                  MonitorAdded, MonitorRemoved, MonitorConfigChanged,
                                  KeyboardLayoutChanged, SafeModeEntered,
                                  NotificationDaemonMissing; updated all format() arms
griddy/src/grid/mod.rs          Added MoveDirectionResult enum (Moved / Refused / MovedDirect);
                                  workspace_has_total_fullscreen() — checks TF stack non-empty;
                                  move_window_direction_ex() — walks direction skipping TF-protected
                                  workspaces for Tiled/Fullscreen movers per §6.7.1;
                                  workspace_name() — returns current name or "";
                                  slot_stack_size() — returns stack depth for a slot
griddy/src/keybind/dispatcher.rs  MoveWindowDirection now calls move_window_direction_ex() and
                                    emits WindowMoveSkipped / WindowMoveRefused IPC events;
                                    WorkspaceSync emits WorkspaceSyncChanged (fixed name);
                                    WorkspaceRename emits correct old/new fields via workspace_name();
                                    MoveWindowTo/MoveWindowToIndex emit WindowMoved events;
                                    State/Slot toggles emit WindowStateChanged + WindowSlotChanged;
                                    Stack operations emit WindowStackChanged via emit_stack_changed();
                                    Added emit_window_state_slot_changed() and emit_stack_changed() helpers
griddy/src/state.rs             GlobalState::new() refactored: struct literal stored in
                                  `griddy_state` variable so SafeModeEntered event can be pushed
                                  before returning Ok(griddy_state); reload_theme() now also
                                  emits ThemeReloaded event alongside ConfigReloaded
griddy/src/handlers/xdg_shell.rs  new_toplevel() now emits WindowPlaced event after WindowOpened
                                    (policy_matched = "default", slot_assigned = current slot)
```

#### Key behaviors implemented

1. **IPC event catalogue complete (§12.2)**: All events from the spec are now represented in the
   `Event` enum with correct wire names and data formats. Previously missing: `workspace_created`,
   `workspace_sync_changed` (was wrongly named `workspace_sync_toggled`), `window_placed`,
   `window_slot_adapted`, `window_state_adapted`, `window_move_skipped`, `window_move_refused`,
   `window_stack_changed`, `theme_reloaded`, `shader_loaded`, `shader_error`, `monitor_added`,
   `monitor_removed`, `monitor_config_changed`, `keyboard_layout_changed`, `safe_mode_entered`,
   `notification_daemon_missing`.

2. **TotalFullscreen workspace protection (§6.7.1)**: `move-window-direction` now walks past
   TF-protected workspaces for Tiled/Fullscreen movers. Floating and TotalFullscreen movers are
   always allowed (skip detection skipped). When a workspace is skipped, `window_move_skipped` IPC
   event is emitted with requested vs actual coords. When all workspaces in the direction are TF-
   protected, `window_move_refused` is emitted with `reason = "all-protected"`.

3. **Event wiring — state/slot changes**: `state-fullscreen-toggle`, `state-total-fullscreen-toggle`,
   `state-floating-toggle`, and all `slot-*` actions now emit `window_state_changed` and
   `window_slot_changed` after every dispatch.

4. **Event wiring — stack changes**: All stack operations (`stack-next`, `stack-prev`, `stack-flip`,
   `stack-promote`, `stack-collapse`, `stack-move-up`, `stack-move-down`) emit `window_stack_changed`
   with slot name, top_id, and stack size.

5. **Event wiring — window moved**: `move-window-to` and `move-window-to-index` emit `window_moved`
   with the target coords.

6. **`window_placed` event (§6.9)**: Emitted in `new_toplevel()` after `window_opened`, recording
   the placement policy and slot for bars/scripts.

7. **`workspace_sync_changed` fix**: Previously emitted `workspace_sync_toggled` (wrong name);
   now emits `workspace_sync_changed` with `mode` value `"synced"` or `"unsynced"` matching spec.

8. **`theme_reloaded` event (§8.5)**: `reload_theme()` now emits `theme_reloaded` in addition to
   `config_reloaded{which:"theme"}`.

9. **`safe_mode_entered` event**: Pushed into pending_events on GlobalState creation when safe mode
   is active.

#### Build status

```
cargo test --workspace   →   36 passed; 0 failed; 0 warnings
```

---

## Session 25 — 2026-05-23

### What was built

**`window_slot_adapted`/`window_state_adapted` event emission, `move-window-to` TF protection, `griddyctl grid resize`, `window_stack_reordered` event, `passthrough`/`repeat` bind flags, griddyctl subcommands**

#### Files updated

```
griddy/src/grid/mod.rs          AddWindowResult struct: carries slot_adapted + state_adapted fields
                                  so adaptation info flows out of add_window(); added slot_stack_ids()
                                  method; added resize_grid() method (add/remove rows/cols at
                                  runtime, clamp out-of-bounds windows, clamp focused workspace,
                                  max 16x16); +5 unit tests (resize_grid_*, slot_stack_ids_*,
                                  workspace_has_total_fullscreen_false_for_empty)
griddy/src/handlers/xdg_shell.rs  Destructures AddWindowResult; emits WindowSlotAdapted and
                                    WindowStateAdapted events when conflict resolver adapts a window
griddy/src/keybind/dispatcher.rs  move_to_tf_protected() helper: MoveWindowToIndex + MoveWindowTo
                                    now refuse to move Tiled/Fullscreen movers into TF-protected
                                    workspaces; emit_stack_reordered() helper: emits
                                    window_stack_reordered after StackMoveUp/StackMoveDown/StackFlip
griddy/src/ipc/events.rs        Added WindowStackReordered { slot, order_csv } variant + format arm;
                                  +4 unit tests (window_slot_adapted_wire, window_state_adapted_wire,
                                  window_stack_reordered_wire, window_move_refused_wire)
griddy/src/ipc/commands.rs      "grid" → grid_cmd(): routes "grid info" to grid_info() and
                                  "grid resize <cols>x<rows>" to grid_resize() via Grid::resize_grid();
                                  "reload" → reload_cmd(): routes "reload theme" to state.reload_theme()
                                  and bare "reload" to reload_config_if_changed(true)
griddy/src/keybind/mod.rs       Added repeat: bool field to BindEntry; from_config() reads
                                  bind.repeat; lookup_flags() return type extended to 5-tuple
                                  (Action, locked, global, passthrough, repeat)
griddy/src/backend/winit.rs     Fixed keyboard filter: destructures 5-tuple from lookup_flags;
                                  passthrough binds dispatch action then return FilterResult::Forward
                                  so the key also reaches the focused client
griddyctl/src/main.rs           Added Grid(GridCommand) → grid resize <cols> <rows> subcommand;
                                  added Workspace(WorkspaceCommand) → workspace apply-template
                                  and workspace rename subcommands; Reload now takes --target
                                  ("all" default, "theme" for theme-only reload)
```

#### Key behaviors implemented

1. **`window_slot_adapted` / `window_state_adapted` events**: `add_window()` now returns `AddWindowResult` carrying `slot_adapted: Option<(Slot, Slot)>` and `state_adapted: Option<(WindowState, WindowState, Option<Slot>)>`. In `new_toplevel()`, these are compared against the original request and the corresponding IPC events emitted. Bars and scripts can now detect when the conflict resolver overrode a placement request.

2. **`move-window-to` TF protection (§6.7.1)**: The `move_to_tf_protected()` helper checks whether the target workspace has a TotalFullscreen window before moving. Tiled and Fullscreen movers are refused with a `window_move_refused` IPC event; Floating and TotalFullscreen movers are allowed through. Applies to both `MoveWindowTo` (absolute coords) and `MoveWindowToIndex` (1–9).

3. **`griddyctl grid resize <cols> <rows>`**: `Grid::resize_grid()` dynamically adds or removes workspace rows/cols. Growing: adds empty workspaces up to max 16×16. Shrinking: windows in removed columns/rows are clamped to the nearest valid cell; focused workspace clamped similarly. Old (cols, rows) returned for event emission. Exposed via `griddyctl grid resize 4 3`.

4. **`window_stack_reordered` event**: New IPC event with `slot` name and comma-separated `order_csv` of window IDs. Emitted after every `stack-move-up`, `stack-move-down`, and `stack-flip` operation via `emit_stack_reordered()`. Clients can now track exact stack order changes (not just "something changed").

5. **`passthrough` bind flag**: `BindEntry.passthrough` wired end-to-end. `lookup_flags()` returns it as the 4th element. The winit keyboard filter checks: if `passthrough = true`, runs the compositor action AND returns `FilterResult::Forward` so the key also reaches the focused Wayland client. Allows binds like `Super+c` that both center a window and send the keypress to an app.

6. **`repeat` bind flag**: `BindEntry` gains a `repeat: bool` field. `from_config()` reads it from the config. `lookup_flags()` returns it as the 5th element. The infrastructure is in place; held-key repeat suppression will be wired in a future session.

7. **`griddyctl reload --target theme`**: `reload_cmd()` routes `"reload theme"` to `state.reload_theme()` (theme.toml only) and bare `"reload"` to full config reload. `griddyctl reload --target theme` sends the correct IPC command.

8. **`griddyctl workspace apply-template / rename`**: Two new `Workspace` subcommands in griddyctl. `apply-template <col> <row>` sends `dispatch apply-template <col>,<row>`. `rename <col> <row> <name>` sends `dispatch workspace-rename <col>,<row> <name>`.

#### Build status

```
cargo test --workspace   →   45 passed; 0 failed; 0 warnings
```

---

## Session 26 — 2026-05-23

### What was built

**`[[bind_release]]` support (§9), key-repeat suppression, stack depth cue (§6.8), overview-peek / stack-peek actions, `wlr-output-management-unstable-v1` stub (§3)**

#### Files created

```
griddy/src/handlers/output_management.rs   WlrOutputManagerState + GlobalDispatch/Dispatch impls
                                             for ZwlrOutputManagerV1/Head/Mode/Configuration/ConfigHead;
                                             advertises one virtual HEADLESS-1 output at 60Hz;
                                             all apply/test requests immediately cancelled
```

#### Files updated

```
griddy/src/config/types.rs          Added bind_releases: Vec<BindConfig> field (renamed "bind_release")
                                      to Config struct; enables [[bind_release]] TOML sections
griddy/src/config/theme.rs          Added StackedConfig struct (depth_cue, depth_offset_px,
                                      depth_max_layers); added stacked: StackedConfig field to
                                      WindowThemeSection
griddy/src/keybind/mod.rs           Added default_releases() constructor to BindTable;
                                      added stack-peek ($mod+grave) to default press binds;
                                      added default_release_binds() with $mod+grave → stack-peek
griddy/src/keybind/dispatcher.rs    Added OverviewPeek + StackPeek to Action enum;
                                      parse_action() entries for both;
                                      dispatch: OverviewPeek toggles is_overview_peeking (enters/exits
                                      peek overview, emits ViewModeChanged); StackPeek toggles
                                      is_stack_peeking
griddy/src/state.rs                 Added is_overview_peeking: bool, is_stack_peeking: bool,
                                      release_table: BindTable, held_keys: HashSet<u32>,
                                      wlr_output_manager_state: Option<WlrOutputManagerState>;
                                      new() builds release_table from bind_releases or defaults;
                                      initializes wlr_output_manager_state;
                                      reload_config_if_changed() rebuilds release_table on reload
griddy/src/handlers/mod.rs          Added pub mod output_management
griddy/src/backend/winit.rs         Key-release block: removes sym from held_keys; if is_stack_peeking
                                      → clears flag + intercepts; checks release_table; else forwards;
                                      Key-repeat suppression: held_keys tracks pressed syms; if sym
                                      already held, checks repeat flag via lookup_flags(); non-repeat
                                      binds filtered out on OS repeat events;
                                      Added stack_depth: usize to RenderItem and WindowRenderData;
                                      Added in_overview = is_overview || is_overview_peeking flag;
                                      Render path uses in_overview instead of is_overview;
                                      Stack depth cue rendering: before window surface, draws
                                      bottom-right edge strips at (depth_offset_px * layer) offset
                                      for each layer below top-of-stack using idle color * 0.85
```

#### Key behaviors implemented

1. **`[[bind_release]]` (§9)**: A separate `release_table: BindTable` in `GlobalState` maps keybinds to actions that fire on key *release* rather than press. Built from `[[bind_release]]` TOML entries; falls back to `default_releases()` if none configured. Key releases update `held_keys` and check the release table.

2. **Key-repeat suppression**: `held_keys: HashSet<u32>` tracks all currently-held keysyms. On key press, if `held_keys` already contains the sym (OS auto-repeat), `lookup_flags()` checks the `repeat: bool` flag. Non-repeat binds return `FilterResult::Forward` without firing. This prevents binds like `overview-toggle` from rapid-firing on repeat events.

3. **`stack-peek` action (§6.8)**: Toggled on press (`$mod+grave`) and cleared on release. `is_stack_peeking = true` while key is held. Any key release while peeking clears the flag. (Visual fan-out rendering of stacked windows planned for a future session.)

4. **`overview-peek` action**: Toggles `is_overview_peeking` — shows the overview grid while a key is held, hides it on release. Emits `ViewModeChanged` events. The `in_overview = is_overview || is_overview_peeking` combined flag drives all overview render paths, so peek uses identical rendering to the toggle overview.

5. **Stack depth cue rendering (§6.8)**: When `theme.window.stacked.depth_cue = true` and a slot has more than one window, edge strips are rendered at the bottom-right of each window behind the top-of-stack surface. Each layer shifts by `depth_offset_px` pixels right and down; renders up to `depth_max_layers` layers using `idle_color * 0.85` alpha.

6. **`wlr-output-management-unstable-v1` stub (§3)**: `WlrOutputManagerState` registers a v4 global. On bind: creates a `ZwlrOutputHeadV1` named "HEADLESS-1" with a single mode matching the current output size at 60 Hz. All `CreateConfiguration` / `Apply` / `Test` requests respond with `cancelled()` — real mode-setting requires the DRM backend. Satisfies kanshi, wlr-randr, and nwg-displays protocol requirements.

#### Smithay notes

| Issue | Resolution |
|-------|-----------|
| `ZwlrOutputHeadV1::scale()` expects `f64` (not `Fixed`/`i32`) | Rust bindings expose as `f64`; use `1.0f64` |
| `ZwlrOutputHeadV1::transform()` expects `wl_output::Transform` enum | Use `Transform::Normal`, not a raw integer |
| `ZwlrOutputConfigurationV1::DisableHead` has no `id` field | Separate `EnableHead { id, .. }` and `DisableHead { .. }` match arms |

#### Build status

```
cargo test --workspace   →   45 passed; 0 failed; 0 warnings
```

## Session 27 — 2026-05-23

### What was built

**Stack-peek cascade rendering (§6.8), FullscreenConfig (§6.7), window opacity/border_color from rules, sticky window rendering, `wlr-screencopy-unstable-v1` stub, `wp-tearing-control-v1` stub, `wp-fifo-v1` stub (§3)**

#### Files created

```
griddy/src/handlers/screencopy.rs     wlr-screencopy-unstable-v1 v3 stub: advertises ARGB8888 buffer
                                       with buffer_done; responds to copy/copy_with_damage with
                                       failed() — EGL glReadPixels deferred to DRM backend
griddy/src/handlers/tearing_control.rs wp-tearing-control-v1 v1 stub: registers manager global;
                                       GetTearingControl → init WpTearingControlV1;
                                       SetPresentationHint logged as no-op in winit
griddy/src/handlers/fifo.rs           wp-fifo-v1 v1 stub: registers manager global;
                                       GetFifo → init WpFifoV1; SetBarrier/WaitBarrier
                                       traced as no-ops in winit backend
```

#### Files updated

```
griddy/src/config/types.rs          Added FullscreenConfig struct (total_release_key: "Escape",
                                      allow_tearing: true, tearing_hides_floating: true);
                                      added fullscreen: FullscreenConfig to Config
griddy/src/config/theme.rs          Extended StackedConfig with peek_style: String ("cascade"),
                                      peek_cascade_offset_px: i32 (28),
                                      peek_dim_unstacked: f32 (0.75)
griddy/src/handlers/mod.rs          Added pub mod screencopy, pub mod tearing_control,
                                      pub mod fifo
griddy/src/state.rs                 Registered TearingControlManagerState, WlrScreencopyState,
                                      WpFifoManagerState in GlobalState::new()
griddy/src/backend/winit.rs         RenderItem: added opacity: f32, border_color: Option<String>;
                                      WindowRenderData: added override_border: Option<[f32;4]>;
                                      Sticky window pass: separate loop over all windows finds
                                        sticky=true windows not on current workspace, adds them
                                        to render items via compute_rect();
                                      Window opacity: anim_alpha * item.opacity clamped to 0..1;
                                      border_color rule override: parse_hex_color → override_border
                                        replaces focused/idle/urgent color in border rendering;
                                      Phase 1e cascade pre-computation: before renderer.render(),
                                        if is_stack_peeking: builds CascadeLayerData vec with
                                        render_elements_from_surface_tree per stack member,
                                        shifted by idx * peek_cascade_offset_px, dimmed by
                                        peek_dim_unstacked for non-top layers;
                                      Cascade overlay: clears screen with 50% black dim,
                                        renders bottom-to-top (.iter().rev()) drawing bg rect,
                                        border rect, then surface elements per layer
```

#### Key behaviors implemented

1. **Stack-peek cascade (§6.8)**: When `$mod+grave` is held (`is_stack_peeking = true`), all windows in the focused slot's stack are displayed as a staircase diagonal. Each window shifts `peek_cascade_offset_px` px right and down per index. Top-of-stack (idx=0) at full opacity; lower layers at `peek_dim_unstacked` alpha. Screen background dims to 50% black. Rendered bottom-to-top (`.iter().rev()`) so top-of-stack appears visually on top.

2. **`FullscreenConfig` (§6.7)**: New `[fullscreen]` config section with `total_release_key` (key that exits TotalFullscreen), `allow_tearing` (opts full-screen windows into tearing presentation), `tearing_hides_floating` (floats hidden during TotalFullscreen). Defaults: Escape / true / true.

3. **Window opacity from rules**: `window.opacity` field (set via `[[rule]]` `opacity =` property) is now multiplied into render alpha. Opacity combines with open-animation alpha: `alpha = anim_alpha * item.opacity`.

4. **Sticky window rendering**: A second loop after the main `visible_windows()` pass iterates all windows; those with `sticky = true` and not already in the render list are added using `compute_rect()`. Sticky windows appear on every workspace.

5. **`border_color` rule override**: Per-window `border_color` from rules is parsed via `parse_hex_color` and stored as `override_border: Option<[f32;4]>` in `WindowRenderData`. Border rendering checks this first, overriding focused/idle/urgent colors.

6. **Protocol stubs (§3)**: `wlr-screencopy-unstable-v1`, `wp-tearing-control-v1`, and `wp-fifo-v1` registered as Wayland globals. Satisfy grim, games/tearing-aware apps, and frame-pacing clients at the protocol handshake level without requiring the DRM backend.

#### Smithay notes

| Issue | Resolution |
|-------|-----------|
| `render_elements_from_surface_tree` borrows `renderer` exclusively | Pre-compute cascade elements before `renderer.render()` (Phase 1e) — the renderer is consumed into the frame on render() |
| Stack render order: idx=0 is top-of-stack | Use `.iter().rev()` to render bottom layers first, top layer last (visually on top) |

#### Build status

```
cargo build   →   0 errors; 0 warnings
cargo test --workspace   →   45 passed; 0 failed
```

---

## Session 28 — 2026-05-23

### What was built

**`wp-fifo-v1` stub (§3), keyboard layout detection (§12.2), notification daemon check (§14.2), mouse bind support (§9.3), pointer-constraint break key (§8.11), gesture bind config (§9.4), OSD workspace/submap indicator (§8.9)**

#### Files created

```
griddy/src/handlers/fifo.rs           wp-fifo-v1 v1 stub: WpFifoManagerState registers global;
                                       GetFifo → init WpFifoV1; SetBarrier/WaitBarrier no-ops
```

#### Files updated

```
griddy/src/handlers/mod.rs          Added pub mod fifo
griddy/src/state.rs                 Registered WpFifoManagerState in GlobalState::new();
                                      Added current_keyboard_layout: String (XKB layout tracking);
                                      Added notification_daemon_checked: bool;
                                      Added constraint_break_key: Option<(u32,u32)>;
                                      Added gesture_table: GestureTable;
                                      Added osd_hide_at: Option<Instant>, osd_kind: Option<String>;
                                      Added show_osd() helper method;
                                      Added parse_constraint_break_key() helper;
                                      Config reload rebuilds gesture_table and constraint_break_key
griddy/src/config/types.rs          Added GestureConfig struct (fingers, direction, action, args);
                                      Added gestures: Vec<GestureConfig> to Config (TOML [[gesture]])
griddy/src/config/theme.rs          Added OsdConfig struct (enabled, position, margin_px,
                                      duration_ms, workspace_indicator, submap_indicator,
                                      reload_indicator, cell_px, cell_gap_px);
                                      Added osd: OsdConfig to ThemeConfig
griddy/src/keybind/mod.rs           Added MouseBindEntry struct + mouse_entries field to BindTable;
                                      Added mouse_key_to_button() mapping Mouse:Left/Right/Middle/
                                        Forward/Back + Scroll:Up/Down/Left/Right to button codes;
                                      from_config() now routes Mouse:/Scroll: keys to mouse_entries;
                                      Added lookup_button(button_code, mods) method;
                                      Added GestureTable struct + GestureEntry;
                                      GestureTable::from_config(), from_defaults(), lookup()
griddy/src/keybind/dispatcher.rs    emit_workspace_changed() now calls state.show_osd("workspace");
                                      Submap enter calls state.show_osd("submap");
                                      SubmapReset clears osd_hide_at/osd_kind
griddy/src/ipc/commands.rs          notify command now checks org.freedesktop.Notifications
                                      via busctl --user ping on first call; emits
                                      NotificationDaemonMissing if unreachable
griddy/src/backend/winit.rs         Keyboard handler: pointer-constraint break key intercept
                                      (deactivates constraint on break combo, before bind table);
                                      Keyboard handler: XKB layout detection after input()
                                      returns — compares layout_name to current_keyboard_layout,
                                      emits KeyboardLayoutChanged on change;
                                      PointerButton: checks keybind_table.lookup_button() before
                                        hardcoded mod+drag handling;
                                      PointerAxis: checks keybind_table.lookup_button() for
                                        Scroll:Up/Down/Left/Right binds before forwarding;
                                      OSD rendering: after cascade overlay, before frame.finish():
                                        draws bg panel + workspace grid cells (focused=accent,
                                        occupied=idle, empty=bg_alt) or submap accent bar;
                                        hidden when osd_hide_at expires
```

#### Key behaviors implemented

1. **`wp-fifo-v1` stub (§3)**: `WpFifoManagerV1` global registered at version 1. `GetFifo` creates a `WpFifoV1`; `SetBarrier`/`WaitBarrier` are traced as no-ops in winit. Frame-pacing clients get a clean handshake.

2. **Keyboard layout detection (§12.2)**: After each key press, `keyboard.with_xkb_state()` reads the active XKB layout name. When it differs from `current_keyboard_layout`, a `KeyboardLayoutChanged { device: "seat0", layout }` IPC event is emitted. Layout is initialized empty until the first key press to avoid spurious events.

3. **Notification daemon check (§14.2)**: The `notify` IPC command now checks `org.freedesktop.Notifications` via `busctl --user ping` on the first call. If unreachable, `NotificationDaemonMissing` is pushed to the event bus. Falls back to "assume present" if `busctl` is not installed.

4. **Mouse bind support (§9.3)**: `BindTable` extended with `mouse_entries`. Key names `Mouse:Left`, `Mouse:Right`, `Mouse:Middle`, `Mouse:Forward`, `Mouse:Back` and `Scroll:Up`/`Down`/`Left`/`Right` in `[[bind]]` sections map to Linux input button codes. `lookup_button()` called on pointer-button press and scroll events.

5. **Pointer-constraint break key (§8.11)**: `input.pointer_constraint_break_key` (default `"Super+Escape"`) is parsed to (keysym, mods_mask) at startup. In the keyboard filter, before any bind lookup, if the break combo matches, `with_pointer_constraint` deactivates the constraint. Cannot be overridden or consumed by apps.

6. **Gesture bind config (§9.4)**: `[[gesture]]` TOML sections parsed into `GestureConfig`. `GestureTable` stores (fingers, direction) → action entries. Defaults: 4-finger up/down = `overview-toggle`; 3-finger left/right = `workspace-right`/`workspace-left`. Ready for libinput dispatch in the DRM backend.

7. **OSD workspace/submap indicator (§8.9)**: `OsdConfig` added to `theme.toml` (`[osd]`). On workspace change, `show_osd("workspace")` starts a timer. OSD renders a compact NxN grid with accent/idle/bg_alt cells. On submap enter, `show_osd("submap")` renders a solid accent bar. Hides automatically at `duration_ms`. Position controlled by `osd.position` (default `top-center`).

#### Build status

```
cargo build   →   0 errors; 0 warnings
cargo test --workspace   →   45 passed; 0 failed
```

---

## Session 29 — 2026-05-23

### What was built

**OSD reload indicator (§8.9), OSD notification-daemon-missing (§14.2), `osd-show` action wired, safe-mode OSD (§22.2), power-profile hook (§22.17), `keyword`/`getoption` extensions, `griddyctl keyword/getoption/notify`, `griddy -d` debug overlay auto-enable (§22.14), 4 new unit tests**

#### Files updated

```
griddy/src/state.rs             show_osd() checks per-kind flags (workspace_indicator,
                                  submap_indicator, reload_indicator) before showing;
                                  reload_config_if_changed(): show_osd("reload-ok") on
                                    success, show_osd("reload-err") on failure;
                                  reload_theme(): same;
                                  GlobalState::new(): safe mode → 30s "safe-mode" OSD
griddy/src/keybind/dispatcher.rs  Action::OsdShow now calls state.show_osd(text);
                                  StateTotalFullscreenToggle + TotalFullscreenExit:
                                    power-profile hook (§22.17) — powerprofilesctl
                                    set performance/balanced when flag enabled
griddy/src/config/types.rs      FullscreenConfig: added performance_on_total_fullscreen:
                                  bool (default false)
griddy/src/ipc/commands.rs      notify_cmd: show_osd("notification-daemon-missing");
                                  keyword_cmd: 6 new runtime-settable keys;
                                  getoption_cmd: same + input.pointer_constraint_break_key
griddy/src/backend/winit.rs     Added ok_color + danger colors; extended OSD to render
                                  colored pills for reload-ok (green), reload-err (red),
                                  notification-daemon-missing (orange/warn), safe-mode
                                  (red), and custom osd-show text (accent)
griddy/src/main.rs              `griddy -d` sets state.debug_overlay = true at startup
griddyctl/src/main.rs           Added Keyword, Getoption, Notify subcommands
griddy/src/ipc/events.rs        +4 unit tests for notification_daemon_missing, safe_mode_entered,
                                  config_error, config_reloaded wire formats
```

#### Key behaviors implemented

1. **OSD reload indicator (§8.9)**: Config reload success/failure now shows a green (`ok`) or red (`danger`) colored pill in the OSD region. Controlled by `osd.reload_indicator = true`. Both `reload_config_if_changed()` and `reload_theme()` are wired.

2. **OSD notification-daemon-missing (§14.2)**: When `notify` IPC command detects no D-Bus notification daemon, shows an orange (`warn`) pill OSD alongside the event emission.

3. **`osd-show` action fully wired**: `osd-show <text>` keybind now shows the OSD with the text as kind (custom accent bar). Previously it only logged.

4. **Safe mode OSD (§22.2)**: On startup in safe mode, a 30-second danger-colored OSD shows to alert the user that their config was skipped.

5. **Power-profile hook (§22.17)**: `[fullscreen] performance_on_total_fullscreen = false` config flag. When enabled, `TotalFullscreen` entry → `powerprofilesctl set performance`; exit → `powerprofilesctl set balanced`. Wired to both toggle and `total-fullscreen-exit`.

6. **`keyword`/`getoption` extensions**: `theme.osd.enabled`, `theme.osd.duration_ms`, `input.warp_cursor_on_focus_change`, `input.warp_cursor_on_workspace_change`, `debug.overlay`, `fullscreen.performance_on_total_fullscreen`, `input.pointer_constraint_break_key` (getoption only).

7. **`griddyctl keyword/getoption/notify`**: Three new griddyctl subcommands for runtime config, option query, and OSD display.

8. **`griddy -d` debug overlay (§22.14)**: Debug flag automatically enables the frame-time overlay from first frame — no need to press `$mod+Shift+d` manually.

#### Build status

```
cargo build   →   0 errors; 0 warnings
cargo test --workspace   →   49 passed; 0 failed
```

---

## Session 30 — 2026-05-23

### What was built

**Missing §9.1 default binds, XCURSOR env vars, `[shaders]` config, screen-shader action, `setprop` extensions, `plugin` IPC, `same_app` placement policy, `keyword` extensions**

#### Files updated

```
griddy/src/keybind/mod.rs        Added $mod+slash → cheatsheet-toggle and $mod+comma →
                                   osd-show "griddy v0.1" to default_binds() (§9.1)
griddy/src/config/mod.rs         apply_env(): exports XCURSOR_THEME and XCURSOR_SIZE
                                   from config.theme.cursor so child processes inherit them
griddy/src/config/types.rs       New ShadersConfig struct [shaders] block (§11.5) with
                                   screen, open, close, move, resize, workspace_slide,
                                   overview_zoom fields; added to Config
griddy/src/state.rs              Added screen_shader: Option<String> field (§11.5);
                                   initialized from config.shaders.screen on startup
griddy/src/keybind/dispatcher.rs Added SetWindowShader(u64, String) and ScreenShader(String)
                                   actions + parsers + handlers; handlers update
                                   window.shader / state.screen_shader and emit ShaderLoaded
griddy/src/ipc/commands.rs       setprop extended: sticky/pin, no_animations, steal_focus,
                                   above_total_fullscreen, opacity, border_color, shader;
                                   shaders_cmd: reports active screen shader;
                                   plugin_cmd: list/load/unload (§13) with plugin-abi feature gate;
                                   keyword_cmd: +shaders.screen, animations.enabled,
                                   animations.close_duration_ms, input.natural_scroll,
                                   input.tap_to_click, input.disable_while_typing,
                                   windows.focus_on_close
griddyctl/src/main.rs            Added Plugin subcommand (list/load/unload)
griddy/src/handlers/xdg_shell.rs same_app placement policy (§6.9): if config.windows.new_window
                                   .same_app == "stack-with-focused" and a same-app window
                                   exists in the focused workspace, hints.slot is set to
                                   its slot so the new window joins the stack;
                                   window_placed event now carries the actual policy name
                                   ("same_app", "rule", or "default")
```

#### Key behaviors implemented

1. **Default keybinds gap closed (§9.1)**: `$mod+slash` → `cheatsheet-toggle` and `$mod+comma` → `osd-show "griddy v0.1"` were missing from `default_binds()`; both added.

2. **XCURSOR env vars (§8 startup)**: `XCURSOR_THEME` and `XCURSOR_SIZE` are now set in `apply_env()` from `theme.cursor.theme` / `theme.cursor.size`, so GTK/Qt/XWayland child processes inherit the correct cursor configuration.

3. **`[shaders]` config block (§11.5)**: `ShadersConfig` struct added to `config/types.rs` with per-event shader path overrides. `shaders.screen` is loaded into `state.screen_shader` at startup.

4. **`set-window-shader` / `screen-shader` dispatch actions (§11)**: New dispatcher actions set `window.shader` or `state.screen_shader` respectively; both emit `ShaderLoaded` events and are also reachable via `griddyctl shader`.

5. **`setprop` expansion**: Supports `sticky`, `pin`, `no_animations`, `steal_focus`, `above_total_fullscreen`, `opacity`, `border_color`, `shader` in addition to `is_urgent`.

6. **`plugin` IPC (§13)**: `plugin list`, `plugin load <path>`, `plugin unload <name>` commands; `griddyctl plugin` subcommand added. Feature-gated on `plugin-abi`.

7. **`same_app` placement policy (§6.9)**: When `windows.new_window.same_app = "stack-with-focused"`, a new window whose `app_id` matches an existing window on the focused workspace is directed to its slot (entering the same stack). `window_placed` event now carries the actual policy name.

8. **`keyword` extensions**: Added runtime-settable keys: `shaders.screen`, `animations.enabled`, `animations.close_duration_ms`, `input.natural_scroll`, `input.tap_to_click`, `input.disable_while_typing`, `windows.focus_on_close`.

#### Build status

```
cargo build   →   0 errors; 0 warnings
cargo test --workspace   →   49 passed; 0 failed
```

---

## Session 31 — 2026-05-23

### What was built

**`workspace_sync` config field, `griddyctl` UX improvements, `keyword`/`getoption` extensions, unit test expansion (+17 tests)**

#### Files updated

```
griddy/src/config/types.rs      New WorkspaceSyncMode enum (Synced|Unsynced); added
                                  workspace_sync field to GridConfig; deserialization tests
                                  for GridConfig, AnimationsConfig, full Config default
griddy/src/state.rs             Initialize state.workspace_synced from
                                  config.grid.workspace_sync at startup; extracted
                                  initial_workspace_synced before struct literal to avoid
                                  borrow-after-move
griddy/src/ipc/commands.rs      keyword_cmd: +grid.workspace_sync (toggles runtime sync +
                                  emits WorkspaceSyncChanged event), +grid.wrap_x,
                                  +grid.wrap_y (call update_from_config to propagate);
                                  getoption_cmd: +grid.workspace_sync, +grid.wrap_x,
                                  +grid.wrap_y, +windows.on_slot_conflict,
                                  +windows.slot_adaptation; new test module: split_verb,
                                  slot_name, state_name unit tests
griddy/src/grid/mod.rs          Navigation boundary + wrap unit tests:
                                  navigate_stops_at_boundary_without_wrap,
                                  navigate_wraps_horizontally/vertically_when_enabled,
                                  grid_resize_preserves_focus_within_bounds,
                                  navigate_right_from_last_col_stops_without_wrap
griddyctl/src/main.rs           New top-level `monitors` subcommand (shorthand for
                                  `get monitors`); new `setprop <id> <prop> <value>`
                                  subcommand; GetCommand: +Layers, +Shaders, +Cursorpos
dist/default.toml               Added workspace_sync = "unsynced" to [grid] block
dist/griddyctl.1                Documented `get layers`, `get shaders`, `get cursorpos`,
                                  `monitors` top-level shorthand
```

#### Key behaviors implemented

1. **`workspace_sync` config field (§5)**: `[grid] workspace_sync = "synced" | "unsynced"` now parsed from TOML. `WorkspaceSyncMode` enum added. `state.workspace_synced` initialized from config at startup so `workspace-sync-toggle` starts from the user's configured mode.

2. **`keyword grid.workspace_sync`**: Runtime toggle of workspace sync mode via IPC; emits `WorkspaceSyncChanged` event immediately.

3. **`keyword grid.wrap_x` / `keyword grid.wrap_y`**: Runtime toggle of wrap behavior; updates grid via `update_from_config`.

4. **`getoption` expansions**: `grid.workspace_sync`, `grid.wrap_x`, `grid.wrap_y`, `windows.on_slot_conflict`, `windows.slot_adaptation` all queryable.

5. **`griddyctl monitors`** (§12.3): Added direct `monitors` subcommand as a shorthand for `get monitors` (spec example shows `griddyctl monitors` without the `get` prefix).

6. **`griddyctl setprop <id> <prop> <value>`** (§12.3): New subcommand wrapping the `setprop` IPC command — no need to use raw dispatch anymore.

7. **`griddyctl get layers/shaders/cursorpos`**: Three new `get` sub-subcommands mapping to the existing IPC endpoints.

8. **Unit test expansion (+17 tests, 49→66)**: Grid navigation boundary/wrap tests; config deserialization tests for `WorkspaceSyncMode`, `AnimationsConfig`, full `Config::default()`; IPC helper tests for `split_verb`, `slot_name`, `state_name`.

#### Build status

```
cargo build   →   0 errors; 0 warnings
cargo test --workspace   →   66 passed; 0 failed
```

---

## Session 32 — 2026-05-23

### What was built

**`view.default_mode` wiring, `startup.exec` wiring, config documentation, unit test expansion (+11 tests, 66→77)**

#### Files updated

```
~/.config/griddy/config.toml    Fixed all broken fields from old generated default:
                                  follow_mouse "always"→"loose", focus_steal_prevention
                                  moved to [windows.new_window], repeat_rate/delay moved
                                  to [input.keyboard], removed default_state/honor_xdg_autostart
                                  from [windows], removed [osd] section
dist/default.toml               Expanded with new documented sections: [view] default_mode +
                                  slide_easing, [input.keyboard] layout/variant/options,
                                  [input.touchpad], [windows.floating], [fullscreen],
                                  [idle] with [[idle.timeout]] examples, [xwayland],
                                  [session]; startup exec vs exec_once distinction documented
griddy/src/state.rs             Wire view.default_mode → is_overview at startup:
                                  extracted initial_overview before struct literal, used as
                                  is_overview initializer; wire startup.exec in
                                  run_startup_cmds() (runs at startup alongside exec_once);
                                  wire startup.exec in reload_config_if_changed() (re-runs
                                  exec commands on every hot-reload, unlike exec_once)
griddy/src/config/types.rs      +11 unit tests: fullscreen_config_defaults,
                                  fullscreen_config_toml_round_trip,
                                  view_config_default_mode_is_focus,
                                  view_config_overview_mode_parses,
                                  startup_config_exec_fields,
                                  session_config_defaults, xwayland_config_defaults,
                                  idle_config_defaults, idle_timeout_parses,
                                  input_follow_mouse_variants,
                                  conflict_policy_kebab_case
```

#### Key behaviors implemented

1. **`view.default_mode = "overview"`**: If set in config, compositor starts with `is_overview = true` — the overview grid is shown immediately on first frame instead of the focused workspace. Uses `initial_overview` extracted before the `GlobalState` struct literal (same pattern as `initial_workspace_synced`).

2. **`startup.exec` distinction from `exec_once`**: `exec_once` runs only at compositor startup (not on reload). `exec` runs at compositor startup AND on every config hot-reload. This matches Hyprland's semantics — useful for stateless wallpaper commands that need to re-run when the config changes.

3. **Config documentation**: `dist/default.toml` now documents all major config sections. Users reading it can discover `[fullscreen]`, `[idle]`, `[xwayland]`, `[session]`, `[input.touchpad]`, `[windows.floating]` without consulting source code.

4. **User config fix**: `~/.config/griddy/config.toml` was generated from the old broken `dist/default.toml` and contained 6 invalid field placements (including `follow_mouse = "always"` which crashed the compositor). All corrected in-place.

#### Build status

```
cargo build --workspace   →   0 errors; 0 warnings
cargo test --workspace    →   77 passed; 0 failed
```
