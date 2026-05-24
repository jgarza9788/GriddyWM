# GriddyWM — Project Specification

A grid-based Wayland window manager / compositor. Workspaces are arranged on a 2D grid you can navigate spatially, with sliding focus animation and a zoomed-out overview. Windows occupy six geometric **tiled slots** (halves and quarters) and can be **promoted** to Fullscreen or TotalFullscreen states without losing their slot assignment. Slot stacks and promotion stacks both support cycling, drag-reorder, and visual peek.

---

## 1. Goals & Non-Goals

### Goals
- **Spatial grid workspaces.** NxN configurable (2x2, 3x3 default, 4x4, 5x5, 10x10).
- **Two view modes.** Focused (one workspace fills screen, slide to switch) + Overview (zoomed-out grid, drag windows between).
- **Six tiled slots + orthogonal states.** HalfLeft/Right, QuarterTL/TR/BL/BR; states `Tiled`/`Floating`/`Fullscreen`/`TotalFullscreen` apply over any slot. Stacking everywhere.
- **First-class TOML config.** Keybinds, animations, rules, shaders.
- **Powerful IPC.** Two-socket model (commands + events), JSON over UNIX domain sockets, plus a `griddyctl` CLI.
- **Shader pipeline.** Per-event animation shaders (open/close/move/resize/overview), per-window GLSL fragment shaders, screen-space post-process shader.
- **Shell-agnostic.** Works with WayBar, Noctalia, DMS, etc. via standard `wlr-layer-shell` + `ext-workspace-v1`.
- **DM-agnostic.** Standard `.desktop` session file for LightDM / SDDM / GDM / greetd.

### Non-Goals (v1)
- X11 root-WM compatibility (Wayland-only; XWayland clients yes).
- i3/Sway config syntax compatibility.
- Built-in shell / bar / launcher (delegated to ecosystem).
- Dynamic / scrollable tiling (intentionally different paradigm).

---

## 2. Comparison to Existing Compositors

| | Hyprland | Niri | Sway | **GriddyWM** |
|---|---|---|---|---|
| Layout | Dynamic tiling | Scrollable | Manual tiling | **Spatial grid** |
| Workspaces | Linear/named | Vertical per-monitor | Linear/named | **2D grid (NxN)** |
| Backend | Aquamarine | Smithay | wlroots | **Smithay** |
| Language | C++ | Rust | C | **Rust** |
| Overview | Plugin | Built-in (25.05+) | No | **Built-in core** |
| Config | hyprlang/lua | KDL | i3 syntax | **TOML** |
| IPC | 2 UNIX sockets | varlink-style socket | i3 IPC | **2 UNIX sockets (Hyprland-like)** |
| Shaders | screen + plugins | open/close/resize | none | **Per-event + per-window + screen** |

GriddyWM borrows the IPC shape from Hyprland, the Smithay foundation and overview UX from Niri, and the grid-of-workspaces model from desktop conventions like GNOME's old workspace grid and macOS Spaces.

---

## 3. Technology Stack

- **Language:** Rust (stable, edition 2024).
- **Compositor framework:** [Smithay](https://github.com/Smithay/smithay) — handles core Wayland protocol, surface trees, seat, DRM/KMS, libinput.
- **Event loop:** `calloop` (Smithay's choice).
- **Renderer:** Smithay's GLES2 renderer with custom shader pipeline layered on top. Vulkan renderer behind a feature flag for later.
- **Config parsing:** `toml` + `serde`.
- **IPC serialization:** `serde_json`.
- **CLI:** `clap` for `griddyctl` and `griddy` itself.
- **Logging:** `tracing` + `tracing-subscriber`.

### Required protocols (server-side)
Core: `wl_compositor`, `wl_subcompositor`, `wl_seat`, `wl_output`, `wl_shm`, `wl_data_device_manager`, `xdg_shell`, `xdg_decoration_unstable_v1`, `xdg_activation_v1`, `xdg_output_unstable_v1`.

Layer/shell: `wlr_layer_shell_v1`, `ext_workspace_v1`, `wlr_foreign_toplevel_management_unstable_v1`, `ext_foreign_toplevel_list_v1`, `wlr_output_management_unstable_v1` (required by kanshi, wlr-randr, nwg-displays).

Capture/portal: `wlr_screencopy_v1`, `ext_image_capture_source_v1`, `ext_image_copy_capture_v1`, `wlr_export_dmabuf_v1`.

Input: `pointer_constraints_v1`, `relative_pointer_v1`, `tablet_v2`, `virtual_keyboard_v1`, `input_method_v2`, `text_input_v3`.

Sync/perf: `linux_dmabuf_v1`, `presentation_time`, `viewporter`, `single_pixel_buffer_v1`, `fractional_scale_v1`, `cursor_shape_v1`, `content_type_v1`, `tearing_control_v1`, `linux_drm_syncobj_v1` (explicit sync).

Security: `security_context_v1`, `ext_session_lock_v1`.

XWayland: optional, gated behind config + feature flag.

---

## 4. Architecture

```
+----------------------------------------------------------------+
|                          griddy (binary)                       |
| +-------------+  +------------+  +----------------+            |
| | Input Mgr   |  | Config Mgr |  | IPC Server     |            |
| | (libinput)  |  | (TOML+hot- |  | 2x UNIX socket |            |
| |             |  |  reload)   |  |  cmd + event   |            |
| +------+------+  +-----+------+  +-------+--------+            |
|        v                v                 v                    |
| +----------------------------------------------------------+   |
| |                    Core State (Arc<RwLock>)              |   |
| |  - Grid (NxN, per-output or shared)                      |   |
| |  - Workspaces[col,row] -> Vec<Window>                    |   |
| |  - Window registry (id -> Window)                        |   |
| |  - Slot occupancy + state stacks per workspace           |   |
| |  - Animation state machine                               |   |
| +----------------------------+-----------------------------+   |
|                              v                                 |
| +-----------+   +---------------------+   +----------------+   |
| | Layout    |   | Renderer            |   | Wayland Server |   |
| | Engine    |-->| GLES2 + shader pipe |<--| (Smithay)      |   |
| | (slots,   |   | per-window FBOs,    |   |                |   |
| |  states,  |   | screen FBO, blur,   |   |                |   |
| |  overview)|   | shadow, animations  |   |                |   |
| +-----------+   +---------------------+   +----------------+   |
+----------------------------------------------------------------+
```

### Process model
Single-process compositor. Plugins are loaded as `cdylib`s through a stable C ABI plugin interface (see §13). IPC is in-process, not a separate daemon.

### Multi-monitor
Each `wl_output` has its own grid by default (configurable to share a single grid across all outputs). Workspaces remember their home output and migrate back on reconnect (Niri-style).

---

## 5. The Grid Model

### Coordinate system
- Origin `(0, 0)` at top-left.
- `(col, row)` indexing.
- For a 3x3 grid, valid coords are `(0,0)..(2,2)`.

### Grid sizes (config)
`2x2` (4), `3x3` (9, **default**), `4x4` (16), `5x5` (25), `10x10` (100). Non-square `NxM` allowed via explicit `[grid] cols=4 rows=3`.

### Navigation semantics
- **Directional:** `workspace-left`, `workspace-right`, `workspace-up`, `workspace-down`.
- **Diagonal directional:** `workspace-nw`, `workspace-ne`, `workspace-sw`, `workspace-se` — move one step diagonally.
- **Absolute:** `workspace 1,2` jumps to `(col=1, row=2)`. If the target is diagonal from the current position, the slide animation moves diagonally in one motion rather than two sequential slides.
- **Linear index:** `workspace-index 5` maps row-major (`5 → (2,1)` on a 3x3).
- **Wrap:** configurable per-axis (`grid.wrap_x`, `grid.wrap_y`). Diagonal wraps both axes simultaneously.
- **History:** `workspace-back`, `workspace-forward` walk a per-session ring buffer.

**Diagonal animation.** When the target workspace is not on the same row or column as the current workspace, the slide animates in the exact direction of the vector from current to target. From `(1,1)` to `(0,0)` on a 3×3: the viewport slides diagonally northwest in one smooth motion. The distance traveled scales proportionally — moving two cells diagonally covers √2 × cell-width in screen space at the same velocity as a one-cell orthogonal move.

### Empty vs occupied
Workspaces always exist (unlike Niri's dynamic vertical strip). All `cols * rows` cells are addressable from boot. Empty cells render the wallpaper / overview placeholder.

### Per-monitor grids
Default `grid.scope = "per-output"`. Alternative `grid.scope = "global"` shares one grid; each output shows the same active workspace (mirror) or its own slice (`global-slice`, advanced).

### Multi-monitor workspace sync

When two or more outputs are connected and `grid.scope = "per-output"`, each monitor has its own independent grid. Workspace sync controls whether navigating workspaces on one monitor also navigates all other monitors to their corresponding grid position.

**Unsynced mode** (`grid.workspace_sync = "unsynced"`, **default**)
Each monitor navigates independently. `workspace-right` on the focused monitor moves only that monitor's active workspace. All other monitors stay where they are. This is the expected behavior for a typical multi-monitor desk setup where the left monitor shows docs and the right monitor shows code — you switch one without disturbing the other.

**Synced mode** (`grid.workspace_sync = "synced"`)
All monitors step together. The monitor containing the focused window is the leader — it determines the direction and target. Every other connected monitor applies the same relative move to its own grid simultaneously. For example, `workspace-right` moves every output one cell to the right in their respective grids. If one monitor's grid is smaller and the move would go out-of-bounds, that monitor wraps (if `grid.wrap_x` is enabled) or stops at its edge while others continue. The slide animation fires on all monitors in unison.

#### Sync scope and focus

The leader is always the monitor that received the input (the monitor containing the currently focused window). Monitors with no focused window are passive followers. In overview mode, the leader is the monitor whose overview is currently active.

#### Per-monitor opt-out

In synced mode, individual monitors can be pinned — they never follow sync navigation and always behave as if unsynced:

```toml
[[monitor]]
name = "HDMI-A-1"
workspace_sync = false   # never follows sync; navigates independently regardless of global mode
```

This is useful for a secondary monitor you want locked to a specific workspace (e.g. a reference monitor always showing workspace (0,0)) while the rest of the setup syncs together.

#### Config

```toml
[grid]
workspace_sync = "unsynced"   # "synced" | "unsynced" (default: unsynced)
```

#### Dispatcher

`workspace-sync-toggle` — flip between synced and unsynced at runtime without a config reload. The new mode persists until the next config reload (which will restore the value from TOML).

```
griddyctl dispatch workspace-sync-toggle
```

#### IPC event

`workspace_sync_changed` — `mode` (`synced` | `unsynced`)

Emitted whenever sync mode changes, whether triggered by the dispatcher or a config reload. Bars and shells can use this to display the current sync state.

---

## 6. Window Model

A window has two independent properties: which **tiled slot** it occupies (a geometric region within a workspace) and what **window state** it's in (how it participates in rendering and Z-order). Earlier drafts conflated these as "modes"; they're now treated as orthogonal axes.

### 6.1 Window identity
Every mapped toplevel gets a stable `WindowId` (u64) used in IPC and rules.

```rust
struct Window {
    id: WindowId,
    app_id: String,
    title: String,
    workspace: (u8, u8),       // grid coords
    current_state: WindowState,        // see §6.3 — what the window is right now
    requested_state: WindowState,      // what the user/rule last asked for; differs if Fullscreen adapted to Tiled (§6.5.1)
    current_slot: Option<Slot>,        // Some(_) iff current_state == Tiled
    requested_slot: Option<Slot>,      // what the user/rule asked for; differs if Half adapted to Quarter
    stack_index: u32,                  // index in slot or promotion stack (0 = top)
    floating_geom: Rect,               // remembered geometry for Floating restore
    rules_applied: Vec<RuleId>,
    shader: Option<ShaderRef>,
    opacity: f32,
    decorations: DecorationState,
}
```

### 6.2 Tiled slots (geometric regions)

A workspace has exactly six tiled slots. Each slot can hold a stack of windows; only the top of the stack renders.

| Slot | Geometry (within workspace usable area) |
|---|---|
| `HalfLeft` | left 50%, full height |
| `HalfRight` | right 50%, full height |
| `QuarterTL` | top-left 50% × 50% |
| `QuarterTR` | top-right 50% × 50% |
| `QuarterBL` | bottom-left 50% × 50% |
| `QuarterBR` | bottom-right 50% × 50% |

**Slot coexistence rules.** Slots on the same vertical side conflict, slots on opposite sides coexist:

| | HL | HR | QTL | QBL | QTR | QBR |
|---|---|---|---|---|---|---|
| **HalfLeft** | — | ✅ | ❌ | ❌ | ✅ | ✅ |
| **HalfRight** | ✅ | — | ✅ | ✅ | ❌ | ❌ |
| **QuarterTL** | ❌ | ✅ | — | ✅ | ✅ | ✅ |
| **QuarterBL** | ❌ | ✅ | ✅ | — | ✅ | ✅ |
| **QuarterTR** | ✅ | ❌ | ✅ | ✅ | — | ✅ |
| **QuarterBR** | ✅ | ❌ | ✅ | ✅ | ✅ | — |

"Coexist" means both slots can be simultaneously occupied (matches the layouts in the original images: HalfLeft + QuarterTR + QuarterBR, etc.). "Conflict" means activating the second slot must resolve against whatever holds the first; see §6.5.

### 6.3 Window states

State determines how a window participates in rendering and Z-order. State is orthogonal to slot membership — promoting a `Tiled` window to `Fullscreen` does **not** vacate its slot; it remembers it for restore.

| State | Geometry | Rendered above | Slot? |
|---|---|---|---|
| `Tiled` | matches assigned slot from §6.2 | layer-shell bottom + background | yes |
| `Floating` | user-set `Rect` | tiled windows | no (Z-order within Floating stack) |
| `Fullscreen` | usable area (respects `wlr-layer-shell` exclusive zones) | floating + tiled, **below `top` layer** | remembered, not vacated |
| `TotalFullscreen` | entire output, 100% | **everything**, including all layer-shell layers | remembered, not vacated |

`Fullscreen` and `TotalFullscreen` are **promotions**, not slots. They render on top of whatever tiled+floating layout already exists in the workspace; the underlying layout is preserved and reappears verbatim when the window is dismissed.

#### Promotion stacks
Each workspace has two promotion stacks separate from the six slot stacks:
- **Fullscreen stack** — windows in `Fullscreen` state. Top renders; rest are hidden.
- **TotalFullscreen stack** — windows in `TotalFullscreen` state. Top renders; rest are hidden. Always above the Fullscreen stack.

`stack-next` / `stack-prev` cycle within whichever stack the focused window currently belongs to (tiled-slot stack, Fullscreen stack, or TotalFullscreen stack).

### 6.4 Z-order (bottom → top)

For a given workspace:

1. Background layer (`wlr-layer-shell` background)
2. Bottom layer
3. **Tiled windows** — top of each occupied slot's stack
4. Top layer (`wlr-layer-shell` top)
5. **Fullscreen stack top** (if any) — covers 1–4
6. Overlay layer
7. **TotalFullscreen stack top** (if any) — covers everything tiled/promoted, including overlay
8. **Floating windows** — render *above* TotalFullscreen **on the same workspace**. A floating window on workspace (0,0) is never visible while you're viewing workspace (1,1) regardless of that workspace's state — workspaces are fully independent. The same-workspace rule means a PiP player or sticky terminal you intentionally placed on the same workspace as a movie stays visible above it; a floating window you forgot about elsewhere stays out of your way.
9. Cursor + drag-icon

#### Floating vs TotalFullscreen tradeoffs

Floating above TotalFullscreen on the same workspace is the default because it's the only configuration that makes PiP, drag-and-drop previews, and password prompts work correctly during fullscreen content. Since it only applies to windows you deliberately placed on the same workspace, it doesn't produce accidental overlap from other workspaces. Two escape valves:

- **Per-window opt-out:** `above_total_fullscreen = false` (rule property) keeps a specific floating window below TotalFullscreen even on the same workspace.
- **Tearing path optimization:** if a TotalFullscreen has `allow_tearing = true` AND `content-type = "game"`, *all* floating windows on that workspace auto-hide (config: `fullscreen.tearing_hides_floating = true`, default `true`). This preserves the zero-overhead direct-scanout path.

### 6.5 Conflicts

When a window's slot assignment would collide with what's already in the workspace, the compositor runs two passes in order: **adaptation** (try to fit non-destructively) then **resolution** (apply the configured conflict policy).

#### 6.5.1 Slot adaptation (first pass)

Before invoking the conflict resolver, the compositor tries to fit the incoming window non-destructively into available space. The rules differ for tiled-slot requests vs Fullscreen-state requests.

**Floating windows do not adapt** — they keep their (x, y, w, h) regardless of what's tiled or promoted in the workspace. They're handled by Z-order, not by slot logic. See §6.4.

##### Adaptation for tiled slots (Half → Quarter)

If the incoming window requests a tiled `Half` slot and only one of that side's two Quarters is occupied, it adapts to the complementary Quarter:

| Incoming slot | Occupied slot | Free slot | → Adapts to |
|---|---|---|---|
| `HalfLeft` | `QuarterTL` (only) | `QuarterBL` | `QuarterBL` |
| `HalfLeft` | `QuarterBL` (only) | `QuarterTL` | `QuarterTL` |
| `HalfRight` | `QuarterTR` (only) | `QuarterBR` | `QuarterBR` |
| `HalfRight` | `QuarterBR` (only) | `QuarterTR` | `QuarterTR` |

The window remembers it *originally requested* the Half. If the blocking Quarter later vacates, the window pops back up to the Half (tracked as `requested_slot` distinct from `current_slot`). Adaptation in the reverse direction (Quarter into a Half-occupied workspace) does **not** apply — the Half geometrically blocks both Quarters.

##### Adaptation for Fullscreen state

A `Fullscreen` state window entering a workspace that already has tiled occupants does **not** automatically promote over them. It first runs a cascade looking for the largest free tiled region, and only falls through to the promotion stack if nothing fits.

**Cascade (first match wins):**

1. **No tiled occupants** → stays as `Fullscreen` state, takes full usable area.
2. **HalfLeft side is fully free** (HalfLeft + QuarterTL + QuarterBL all empty, but right side has at least one occupant) → adapts to `Tiled / HalfLeft`.
3. **HalfRight side is fully free** (HalfRight + QuarterTR + QuarterBR all empty, but left side has at least one occupant) → adapts to `Tiled / HalfRight`.
4. **At least one Quarter is free** → adapts to the first free Quarter in reading order: `QuarterTL` → `QuarterTR` → `QuarterBL` → `QuarterBR`.
5. **All four Quarters are occupied** → still adapts to a Quarter; **never falls through to the promotion stack by default**. Target Quarter is determined by how the window arrived:
   - **Dragged in overview** → the Quarter the user dropped it onto.
   - **Command or keybind** → `QuarterTL` by default. Override by passing a hint: `move-window-direction right --slot quarter-tr`, or using `slot-quarter-tr` directly after moving.
   - The displaced occupant of that Quarter is handled by the normal conflict resolver (§6.5.2): stack, swap, or kick-to-floating per config.

Falling through to the Fullscreen promotion stack is opt-in via `windows.fullscreen_adaptation_stack_fallback = true` (default `false`). Enable it if you want the old "cover everything" behavior when the workspace is fully tiled.

A Fullscreen-state window that adapted to a tiled slot still remembers `requested_state = Fullscreen`. If the workspace later empties out enough to host a full Fullscreen (all tiled slots that would block become free), the window auto-promotes back. Restoration is opt-in via `windows.fullscreen_auto_restore = true` (default `true`).

##### Adaptation for TotalFullscreen state

`TotalFullscreen` **never adapts**. It's the strongest promotion (intended for movies, games, immersive fullscreen) and always renders above all tiled/floating content on its workspace. If the user explicitly requests `TotalFullscreen`, that's what they get — no clever downgrading.

##### Config

```toml
[windows]
slot_adaptation                      = true   # Half → complementary Quarter (above)
fullscreen_adaptation                = true   # Fullscreen → largest available tiled slot (above)
fullscreen_adaptation_stack_fallback = false  # if true, Fullscreen promotes when all 4 quarters taken instead of stacking into a Quarter
fullscreen_auto_restore              = true   # auto-promote back when workspace clears
```

Adaptation emits `window_slot_adapted` (for Half → Quarter) or `window_state_adapted` (for Fullscreen → Tiled) IPC events with `id, requested, actual` so bars/shells can show the substitution.

#### 6.5.2 Conflict resolution (second pass)

If adaptation didn't resolve the situation, the conflict resolver fires per `windows.on_slot_conflict = "stack" | "swap" | "kick-to-floating"` unless overridden by a rule.

**Intra-slot conflict** — a window wants slot `S` and `S` is already occupied.
- `stack` (default): incoming window joins the top of `S`'s stack; previous top demotes one rank.
- `swap`: incoming becomes top of `S`; previous top moves to incoming's source slot (or to Floating if incoming had none).
- `kick-to-floating`: incoming arrives as `Floating` instead, with default size.

**Inter-slot conflict** — a window wants slot `S₁` that geometrically conflicts with occupied slot(s) `S₂` (per the matrix in §6.2) and adaptation didn't apply. The compositor treats the blocking set as the conflict target:
- `stack` (default): incoming joins the top of the blocker's stack and is tagged as wanting `S₁` on restore. If the blocker later vacates, the next-tagged window in the stack pops back to its original slot tag.
- `swap`: evicts all windows currently in the blocking set into Floating, then takes `S₁`.
- `kick-to-floating`: incoming arrives as `Floating`.

**Promotion has no slot conflict.** Moving a window into `Fullscreen` or `TotalFullscreen` never conflicts with tiled slots — it stacks onto the workspace's promotion stack. Demoting back to `Tiled` reuses the remembered slot; if that slot is now occupied, the full two-pass flow (adaptation → conflict resolution) applies.

### 6.6 Worked examples

#### Scenario A — Fullscreen arrives in a fully-tiled workspace (all 4 quarters taken)

Destination workspace: `HalfLeft` (A), `QuarterTR` (B), `QuarterBR` (C). All four geometric left-side slots are equivalent to left-half, so all 4 quarters are effectively occupied. A `Fullscreen` window D is dragged via overview onto the `QuarterTR` thumbnail.

| Step | Outcome |
|---|---|
| D arrives requesting Fullscreen | Cascade steps 2–4: all 4 quarters occupied (HalfLeft blocks TL+BL, QuarterTR + QuarterBR taken). Step 5 fires: user dragged to `QuarterTR` → D adapts to `Tiled / QuarterTR`. Emits `window_state_adapted id=D requested_state=Fullscreen actual_state=Tiled actual_slot=QuarterTR`. |
| B (previous QuarterTR occupant) | Displaced — conflict resolver fires: `on_slot_conflict = "stack"` → B joins D's QuarterTR stack at rank 1. |
| D moved by keybind (no drag) | Step 5 default: adapts to `QuarterTL`. If TL is blocked by HalfLeft, `QuarterTL` is inaccessible so the default climbs reading order: `QuarterTL` (blocked) → `QuarterTR` → takes `QuarterTR` as the nearest override. |
| D closes | Promotion stack: empty. B pops back to top of QuarterTR. A/B/C layout intact. |

#### Scenario B — Fullscreen adapts to HalfRight

Destination workspace has only `HalfLeft` (A) occupied. A `Fullscreen` window D arrives.

| Step | Outcome |
|---|---|
| D arrives requesting Fullscreen | Cascade step 3: HalfRight side fully free → D adapts to `Tiled / HalfRight`. `requested_state = Fullscreen`, `current_state = Tiled`, `current_slot = HalfRight`. Emits `window_state_adapted id=D requested_state=Fullscreen actual_state=Tiled actual_slot=HalfRight`. |
| A closes | Workspace now holds only D in HalfRight. `fullscreen_auto_restore = true` (default) → D auto-promotes back to `Fullscreen`. Emits `window_state_changed id=D state=Fullscreen`. |

#### Scenario C — Fullscreen adapts to QuarterBR

Destination workspace has `HalfLeft` (A) and `QuarterTR` (B). A `Fullscreen` window D arrives.

| Step | Outcome |
|---|---|
| D arrives requesting Fullscreen | Cascade: HalfLeft side blocked, HalfRight side blocked (QuarterTR occupied). Step 4: first free Quarter in reading order is `QuarterBR` (TL blocked by HalfLeft, TR occupied, BL blocked by HalfLeft) → D adapts to `Tiled / QuarterBR`. Layout now matches image 4. |

#### Scenario D — Half adapts to complementary Quarter

Destination workspace has `QuarterTL` (A). A `HalfLeft` window F arrives.

| Step | Outcome |
|---|---|
| F arrives requesting HalfLeft | Adaptation: QuarterTL occupied, QuarterBL free → F adapts to `Tiled / QuarterBL`. `requested_slot = HalfLeft`. Emits `window_slot_adapted`. |
| A closes | F's `requested_slot` is checked; HalfLeft now possible → F pops back to `HalfLeft`. |

#### Scenario E — Move skips TotalFullscreen workspace

User is on workspace `(0,1)` with a focused `Tiled / HalfLeft` window F. Workspace `(1,1)` has an active `TotalFullscreen` window G (movie playing). Workspace `(2,1)` is empty. User runs `move-window-direction right`.

| Step | Outcome |
|---|---|
| Move evaluates target `(1,1)` | Workspace is TotalFullscreen-protected. F is `Tiled` (per §6.7.1 table, Tiled is skipped). |
| Walk continues right | Target `(2,1)`. Not protected. F lands in `(2,1)`. Emits `window_move_skipped id=F requested=1,1 actual=2,1`. |
| Movie keeps playing | G is undisturbed in `(1,1)`. |

#### Scenario F — Floating shares workspace with TotalFullscreen

Workspace `(1,1)` has TotalFullscreen G (game, `allow_tearing = true`, `content-type = "game"`). User wants to drop a floating PiP H into `(1,1)`.

| Step | Outcome |
|---|---|
| User runs `move-window-to 1,1` on H (Floating state) | Per §6.7.1, Floating is allowed onto protected workspaces. H lands in `(1,1)`. |
| Render check: G has `tearing_hides_floating = true` (default) | H is hidden during G's tearing fullscreen. Compositor preserves direct-scanout tearing path. |
| User exits TotalFullscreen on G | H reappears, floating above the now-tiled/empty workspace. |
| Variant: G is a movie (not a game, no tearing) | H renders above G normally (§6.4 Z-order) — classic PiP behavior. |

### 6.7 Moving windows between workspaces

- `move-window-to col,row` — instant. Slot/state preserved if possible; conflicts resolved per §6.5.
- `move-window-direction left|right|up|down` — adjacent grid cell.
- Overview drag-drop — pointer-driven; dropping onto an occupied slot triggers the conflict resolver.
- `move-window-to-workspace-and-follow` — moves and changes focus.

#### 6.7.1 TotalFullscreen-protected workspaces

A workspace with an active `TotalFullscreen` window is "protected" from incoming Tiled and Fullscreen-state windows — the user is watching a movie / playing a game and shouldn't have surprise windows appear underneath the fullscreen. The rule:

| Mover's state | Behavior on move to TotalFullscreen workspace |
|---|---|
| `Tiled` | **Skipped.** The move continues past the protected workspace. |
| `Fullscreen` | **Skipped.** Same reason — would just join the promotion stack invisibly. |
| `Floating` | **Allowed.** Floating windows can share workspaces with TotalFullscreen and render above it (§6.4). |
| `TotalFullscreen` | **Allowed.** Joins the target workspace's TotalFullscreen stack; `stack-next` cycles within it. |

##### Directional moves
`move-window-direction <dir>` walks in the requested direction, skipping any protected workspace, until it finds:
- A non-protected workspace → window lands there.
- The grid edge → wraps if `grid.wrap_<axis>` is enabled, then keeps walking; otherwise refuses and emits OSD "no non-protected workspace in this direction".

If every workspace in the walk direction is protected (extreme edge case), the move is refused with `window_move_refused` IPC event carrying reason `all-protected`.

##### Absolute moves
`move-window-to col,row` to a protected workspace is refused by default — the user named a specific cell, the compositor shouldn't reinterpret it. OSD shows "Workspace (col,row) is in TotalFullscreen". The window stays put.

Optionally, set `windows.absolute_move_skip = true` to make absolute moves also skip in row-major order to the next non-protected workspace (off by default).

##### Drag-drop in overview
Dragging a Tiled or Fullscreen window onto a protected workspace thumbnail in Overview mode: the thumbnail visually rejects the drop (red border tint) and the drag returns the window to its source. Floating and TotalFullscreen drops work normally.

##### Config

```toml
[windows]
totalfullscreen_protects_workspace = true   # the skip behavior above; default on
absolute_move_skip                 = false  # if true, absolute moves also skip; default off
```

##### IPC events
- `window_move_skipped` — `id, requested_col, requested_row, actual_col, actual_row`
- `window_move_refused` — `id, requested_col, requested_row, reason`

### 6.8 Stack appearance

#### In focus mode — the deck

The top window fills its slot completely. Windows beneath it are not hidden entirely — their back edges peek out from the **bottom-right corner** of the slot, like a deck of cards. Each layer offsets by `stack_depth_offset_px` (default 4px) both right and down, clipped to the slot boundary. A subtle drop shadow between layers reinforces the physical depth. Only the top 3 layers render depth cues regardless of total stack size.

The **stack indicator pill** sits in a configurable corner (default: top-right). It shows current position and total: `2 / 4`. The pill is:
- Rounded rectangle, accent-colored background, white text
- Sized to content, min-width ~48px
- Semi-transparent when the cursor is away; fully opaque on hover

#### Pill scroll interaction (focus mode only)

When the cursor is **directly over the pill**:
- **Scroll down** → `stack-next` (go deeper into the stack)
- **Scroll up** → `stack-prev` (come back toward the top)

The window directly beneath the cursor will contain the previous content — scrolling there scrolls that window normally. The pill is the only hit target for stack scrolling, so content scrolling is never accidentally hijacked.

The transition between stack windows uses the `stack.glsl` animation (cross-fade by default). The pill updates in place: `2 / 4` → `3 / 4`.

```toml
[windows]
stack_scroll_direction = "natural"   # "natural" (down=next, up=prev) | "inverted"
```

#### Stack-peek mode

While `stack-peek` is held (`$mod+grave`), the slot expands into a preview of all stacked windows. Three styles, configurable:

**`cascade`** (default) — windows staircase diagonally from top-left to bottom-right, each shifted `peek_cascade_offset_px` right and down from the one above. All windows are visible and readable. The focused candidate is highlighted with the accent border; others are dimmed to `peek_unstacked` opacity.

**`fan`** — windows radiate outward from the slot center at slight angles (±5°, ±10°, ±15°... per window). Each is scaled down slightly. Good for small stacks (2–3 windows). Becomes crowded above 4.

**`grid`** — for stacks of 4 or more. Windows tile into equal cells within the slot area. Most information-dense; good for larger stacks.

All three modes: scroll wheel or arrow keys traverse candidates. Releasing `stack-peek` activates the highlighted window. `Escape` collapses without changing the active window.

#### In overview mode

Stacked slots show only the top window's thumbnail. The indicator pill appears in the thumbnail corner with the stack count. **Clicking the pill** expands the stack into an inline cascade within the workspace thumbnail; users can drag entries to reorder or drag them out to other workspaces. Overview does **not** support pill-scroll (§ confirmed: focus mode only).

#### Dispatcher table

| Dispatcher | Effect |
|---|---|
| `stack-next` | Cycle to next window in current stack |
| `stack-prev` | Cycle to previous |
| `stack-promote` | Move focused window to top of its stack |
| `stack-collapse` | Eject focused window from stack into `Floating` |
| `stack-move-up` | Reorder: swap with neighbor above |
| `stack-move-down` | Reorder: swap with neighbor below |
| `stack-peek` | Hold-bind: expand stack preview; release activates highlighted |
| `stack-flip <n>` | Jump directly to stack index `n` (0 = top) |

**Drag-and-drop reorder.** In **Overview mode**, clicking the pill expands the stack inline; drag to reorder or move to other workspaces. In **Focus mode**, `$mod`+middle-drag on a stacked window's decoration edge triggers the same expand-and-reorder for the focused stack only.

Stack order persists across reload and is exposed in IPC as `windows[i].stack_index` (0 = top) plus a `stack_kind` field (`tiled-slot`, `fullscreen`, `total-fullscreen`).

### 6.9 New window placement policy

When a new window maps, GriddyWM determines its initial slot/state by checking rules (§10) first, then falling through to this policy. Applies **before** the adaptation cascade — whatever slot the policy picks goes through adaptation if needed.

The policy keys on the window's relationship to what's already open:

| Relationship | Detection | Default behavior |
|---|---|---|
| **Transient** | `xdg_toplevel.set_parent` is set (dialogs, popovers-as-toplevels, "Save As", color pickers) | `Floating`, centered on parent window. If parent is not visible (behind a promotion), centers on screen. |
| **Same app** | Same `app_id` as an existing window | `next-empty-slot` — scan slot order, take first unoccupied slot |
| **PID child** | Process is a child of the focused window's PID | `default` — falls through to default policy. PID-child detection is opt-in per-rule only (`new_window.pid_child = "stack-with-parent"`) due to unreliability under D-Bus activation and sandboxing. |
| **Default** | Everything else | `next-empty-slot` |

**`next-empty-slot` scan order:** `Fullscreen` (if workspace is empty) → `HalfLeft` → `HalfRight` → skip any quarter that geometrically conflicts with occupied slots → `QuarterTL` → `QuarterTR` → `QuarterBL` → `QuarterBR`.

The key rule: **if the workspace is empty, the first window always opens as `Fullscreen` state** and takes the full usable area. This is the natural expectation — you launch an app and it fills the screen. When a second window arrives, the Fullscreen adaptation cascade (§6.5.1) kicks in: the first window adapts down to `HalfLeft`, the second lands in `HalfRight`, giving you the 50/50 split without any manual rearrangement. A third window finds one side open, adapts to the available Quarter.

If all six tiled slots are occupied, triggers the Fullscreen adaptation cascade with the new window treated as requesting `Fullscreen` — meaning it adapts to the largest free region or, in a fully-tiled workspace, to the Quarter it was dropped on or `QuarterTL` by default.

**Dispatchers for explicit placement:**
- `spawn-stacked <cmd>` — opens in the current slot's stack. If the focused window is Floating (no slot), falls back to `spawn-floating`.
- `spawn-in-slot <slot> <cmd>` — opens in a specific slot, triggering adaptation if needed.
- `spawn-floating <cmd>` — opens as Floating regardless of policy.
- `spawn-on-workspace <col,row> <cmd>` — opens on a specific workspace.

**Config:**

```toml
[windows.new_window]
transient        = "floating-on-parent"   # floating-on-parent | next-empty-slot | stack-with-parent
same_app         = "next-empty-slot"      # next-empty-slot | stack-with-focused | floating
default          = "next-empty-slot"      # next-empty-slot | floating | stack-with-focused
focus_on_open    = true                   # give focus to newly opened windows
focus_steal_prevention = true             # windows without a valid xdg_activation token get urgent instead of focus
```

**IPC event:** `window_placed` — `id, policy_matched, slot_assigned`.

### 6.10 Focus-on-close policy

When the focused window closes, GriddyWM must choose what to focus next. Policy applied in order:

1. **Last-focused** (default) — the most recent window in the global focus history ring that is still open and on the current workspace. This matches the Alt+Tab mental model: you go back to what you were doing.
2. **Stack-next** — if the closed window was in a stack, the next window in that stack (now top-of-stack) gets focus.
3. **Slot-neighbor** — if the slot is now empty, focus the nearest occupied slot by spatial proximity (same adjacency model as §6.11).
4. **None** — no window on this workspace gets focus. Focus moves to the workspace's "background" (useful for terminals that auto-exit).

The default chain: last-focused → stack-next → slot-neighbor → none.

```toml
[windows]
focus_on_close = "last-focused"   # last-focused | stack-next | slot-neighbor | none
```

Focus history is per-workspace. Closing the last window on a workspace moves focus to the last-focused workspace (the workspace history ring from §5).

### 6.11 Focus navigation (spatial adjacency model)

`focus-left`, `focus-right`, `focus-up`, `focus-down` move focus between windows within a workspace using geometric proximity. The algorithm:

1. Compute the bounding-box center of the currently focused slot.
2. Find all occupied slots whose centers lie in the requested direction (i.e., for `focus-right`: center.x > focused.center.x).
3. Among those, pick the one whose center is geometrically nearest (Euclidean distance).
4. If no candidates exist **within the workspace**, behaviour depends on `input.cross_workspace_focus`:
   - `true` (default): focus moves to the adjacent workspace in that direction — the same as running `workspace-direction <dir>`. This makes window focus and workspace navigation feel seamless; pressing `$mod+h` always moves left, whether that means another window or another workspace.
   - `false`: focus stops at the workspace boundary. Nothing happens if no window is in the requested direction. Use this if you want explicit workspace navigation and don't want accidental workspace switches.
5. On arrival in a new workspace, focus lands on the last-focused window in that workspace.

**Tie-breaking** (equal distance): reading order — TL > TR > BL > BR > HalfLeft > HalfRight.

**Floating windows** participate in focus navigation by their center position, competing on equal footing with tiled slots.

**Promotion stacks** (Fullscreen, TotalFullscreen): `focus-direction` within a workspace always targets tiled/floating first. To reach a Fullscreen window underneath TotalFullscreen, use `stack-next` or click in overview.

**Examples for a HalfLeft + QuarterTR + QuarterBR layout:**

| Focused | Direction | Result |
|---|---|---|
| HalfLeft | right | QuarterTR (nearest right-side center, upper) |
| HalfLeft | up | workspace above |
| HalfLeft | down | workspace below |
| QuarterTR | left | HalfLeft |
| QuarterTR | down | QuarterBR |
| QuarterBR | up | QuarterTR |
| QuarterBR | left | HalfLeft |

### 6.12 Screen-edge snap (drag-to-slot)

Dragging a **Floating** window within `edge_snap_threshold_px` of a screen edge or corner triggers a slot-assignment preview. Releasing the drag completes the assignment; pressing `Escape` mid-drag cancels.

| Drop region | Assigned slot |
|---|---|
| Left edge (center 1/3 of screen height) | `HalfLeft` |
| Right edge (center 1/3) | `HalfRight` |
| Top-left corner | `QuarterTL` |
| Top-right corner | `QuarterTR` |
| Bottom-left corner | `QuarterBL` |
| Bottom-right corner | `QuarterBR` |
| Center (away from edges) | no snap; window stays Floating |

While hovering near an edge, a translucent slot preview renders at the target geometry. The preview uses the same shader as window-open animations.

Slot conflicts triggered by edge-snap go through the normal adaptation cascade + conflict resolver (§6.5).

**Reverse:** dragging a **Tiled** window away from its slot (drag distance > `unsnap_threshold_px`) promotes it to `Floating`. This is the primary mouse-driven way to un-tile a window.

```toml
[windows]
edge_snap             = true     # enable drag-to-slot
edge_snap_threshold_px = 24
unsnap_threshold_px    = 40      # drag distance to un-tile a tiled window
edge_snap_preview     = true     # show translucent slot preview while dragging
```

### 6.13 Window size constraints

`xdg_toplevel` clients send `min_size` and `max_size` hints. Tiled slots may be smaller than a window's `min_size` (e.g. a Quarter on a 1080p monitor might be 960×540, but an IDE enforces min 800×600 — fine — but a chat app might enforce min 1200×800, which doesn't fit).

**Policy (`windows.min_size_overflow`):**

| Policy | Behavior |
|---|---|
| `float` (default) | Window is placed as `Floating` at its minimum size, centered in the slot area. `current_state` becomes `Floating`; `requested_slot` is remembered for when the slot grows (multi-monitor move, layout change). IPC event: `window_size_constraint_forced_float`. |
| `clip` | Window is placed in the slot at the slot's size regardless of `min_size`. The compositor lies to the client (sends slot size as the configure size). Some apps render incorrectly. Use only for broken apps via per-rule override. |
| `ignore` | Honor the min_size; the window overflows its slot geometry and overlaps neighbors. |

`max_size` is always honored for Floating windows. For tiled slots, `max_size` is ignored — the window fills the slot.

```toml
[windows]
min_size_overflow = "float"    # float | clip | ignore
```

Per-rule override: `min_size_overflow = "clip"` on a specific `app_id`.

**IPC event:** `window_size_constraint_forced_float` — `id, slot, min_w, min_h, slot_w, slot_h`.

---

## 7. View Modes

### 7.1 Focus mode (default)
One workspace fills the screen 1:1. Switching workspaces uses a slide animation:
- Direction inferred from grid delta (current col,row → target col,row).
- **Diagonal moves slide diagonally** in one motion. The viewport travels in the exact direction of the vector from current to target workspace — no two-step sequences.
- The seam between workspaces during the slide is controlled by `gaps.workspaces.slide_gap_px`.
- Animation duration, easing, and shader (§11) all configurable.

### 7.2 Overview mode
Zoomed-out grid view showing all workspaces simultaneously as live thumbnails.

- **Entry:** keybind (`$mod+o`), hot-corner, or 4-finger touchpad swipe up.
- **Exit:** same triggers, `Escape`, or activating a workspace.

#### Mouse interaction
- **Click a workspace** → enter focus mode on that workspace.
- **Click a window inside a thumbnail** → switch to that workspace and focus that window.
- **Drag a window from one workspace thumbnail to another** → moves the window to the target workspace. Drop onto empty space = window keeps its current slot. Drop onto an occupied slot = conflict resolver (§6.5) fires.
- **Drag between stack badges** → reorder windows within or between stacks.
- **Right-drag on empty space** → pan the overview grid (for 4×4 and larger).
- **Scroll** → scroll the overview grid when it exceeds viewport.

#### Keyboard interaction
Arrow keys move focus between workspace thumbnails. The focused workspace is highlighted with a border. Within a focused thumbnail:
- `Tab` / `Shift+Tab` — cycle focus between individual windows within the thumbnail.
- `Enter` — activate: exit overview and switch to the focused workspace (or the focused window within it).
- `Escape` — exit overview, return to previously focused workspace.
- `Space` — "pick up" the focused window. Arrow keys then move it to adjacent workspace thumbnails. `Enter` drops it. `Escape` returns it.

```toml
# Already covered by [[bind]] entries in keybinds.toml:
# overview-focus-direction left|right|up|down  → arrow keys
# overview-activate                            → Enter
# overview-window-next / overview-window-prev  → Tab / Shift+Tab
# overview-grab-window                         → Space (pick up focused window)
```

#### Live thumbnails
Each workspace thumbnail samples at 10 Hz idle / 60 Hz if it's the last-focused workspace. Window titles appear below each thumbnail window when `show_titles = true`.

### 7.3 Transitions
Focus → Overview: shader-driven zoom-out (§11.5). Overview → Focus: inverse.

### 7.4 Minimap

A persistent corner HUD showing the full NxN grid at a glance. Each cell renders as a small rectangle; the focused workspace is highlighted; cells with windows show a dot-badge or fill tint.

- **Default:** off for grids ≤ 3×3 (overview is sufficient); **on** for grids ≥ 4×4.
- **Interaction:** click any cell → navigate to that workspace. Hover → tooltip with workspace name and window count.
- **Rendered as a layer-shell `overlay` surface** — always visible, even during Fullscreen state. Hidden during TotalFullscreen (respects the TF contract).
- Controlled via `griddyctl dispatch minimap-toggle` or the keybind `$mod+m` (default).

```toml
[minimap]
enabled          = "auto"       # true | false | "auto" (on for ≥4×4 grids)
position         = "bottom-right"  # top-left | top-right | bottom-left | bottom-right
margin_px        = 16
cell_px          = 14           # size of each workspace cell in the minimap
cell_gap_px      = 3
bg_color         = "{bg_alt}cc"
focused_color    = "{accent}"
occupied_color   = "{fg_dim}88"
empty_color      = "{bg}88"
rounded_px       = 4
opacity          = 0.85
hide_in_overview = true         # redundant during overview; auto-hide
```

---

## 8. Configuration

### File locations (resolved in order)
1. `$GRIDDY_CONFIG` env var
2. `$XDG_CONFIG_HOME/griddy/config.toml`
3. `~/.config/griddy/config.toml`
4. `/etc/griddy/config.toml`

Multi-file include via `imports = ["keybinds.toml", "rules.toml"]`.

### Hot reload
File watched via `inotify`. Save → re-parse → diff-apply. Bad config never crashes the compositor; previous-good config is retained and an `error` event is emitted on the IPC bus.

### Schema (annotated example)

```toml
# ~/.config/griddy/config.toml

[grid]
cols = 3
rows = 3
scope = "per-output"      # "per-output" | "global" | "global-slice"
wrap_x = false
wrap_y = false
default = [1, 1]          # workspace focused at startup (center)
workspace_sync = "unsynced"   # "synced" | "unsynced" — whether workspace navigation on one monitor moves all monitors (§5)

[view]
default_mode = "focus"    # "focus" | "overview"
slide_duration_ms = 220
slide_easing = "ease-out-cubic"
# Diagonal moves always slide diagonally in one motion (see §5 Navigation semantics)

[overview]
hot_corner = "top-left"   # "top-left"|"top-right"|"bottom-left"|"bottom-right"|"none"
hot_corner_delay_ms = 150
scale = 0.18              # thumbnail scale factor (auto-computed if 0)
# Thumbnail gap: controlled by gaps.workspaces.overview_gap_px in theme.toml
zoom_duration_ms = 280
zoom_easing = "ease-in-out-cubic"
thumbnail_fps_idle = 10
thumbnail_fps_active = 60
show_titles = true
show_labels = true        # workspace user labels
dim_unfocused = 0.85

[windows]
on_slot_conflict = "stack"            # "stack" | "swap" | "kick-to-floating"
slot_adaptation = true                # Half adapts into available complementary Quarter (§6.5.1)
fullscreen_adaptation = true          # Fullscreen adapts to largest free tiled slot (§6.5.1)
fullscreen_auto_restore = true        # adapted Fullscreen pops back when workspace clears
totalfullscreen_protects_workspace = true  # Tiled/Fullscreen moves skip past TF workspaces (§6.7.1)
absolute_move_skip = false            # whether absolute moves also skip (default refuse)
stack_scroll_direction = "natural"    # "natural" (scroll down = stack-next, up = stack-prev) | "inverted"
# When focus moves to a window beneath a Fullscreen-promoted window,
# auto-demote the Fullscreen so the focused window becomes visible.
focus_steals_demotes = true

[fullscreen]
total_release_key = "Escape"          # key to leave total-fullscreen
allow_tearing = true                  # opt-in tearing_control_v1 for games
tearing_hides_floating = true         # hide floating windows when a tearing TF is active

[windows.floating]
default_size = [800, 600]
center_on_open = true
above_total_fullscreen = true         # floating renders above TF by default (§6.4)
# Appearance (gaps, borders, shadows, opacity) lives in theme.toml (§8.5)

[animations]
enabled = true
open = "scale-fade"                   # name from [animations.curves]
close = "scale-fade"
move = "spring"
resize = "ease-out-cubic"
workspace_slide = "ease-out-cubic"
overview_zoom = "ease-in-out-cubic"

[animations.curves.spring]
type = "spring"
damping = 0.82
stiffness = 350

[animations.curves.scale-fade]
type = "bezier"
points = [0.16, 1.0, 0.3, 1.0]
duration_ms = 200

[shaders]
screen = ""                           # global post-process .glsl path; empty = none
open = ""                             # animation shaders override built-in
close = ""
move = ""
resize = ""
workspace_slide = ""
overview_zoom = ""

[input]
mod_key = "Super"
follow_mouse = "loose"                # "off" | "loose" | "strict"
cross_workspace_focus = true          # focus-direction at workspace edge navigates to adjacent workspace (§6.11)
                                      # set false to stop focus at workspace boundaries
double_click_ms = 250
natural_scroll = true
tap_to_click = true

[input.keyboard]
layout = "us"
variant = ""
options = "ctrl:nocaps"
repeat_rate = 35
repeat_delay = 400

[input.touchpad]
disable_while_typing = true
scroll_method = "two-finger"

[startup]
exec_once = [
    "waybar",
    "swww-daemon",
    "mako",
    "hyprpolkitagent",
    "wl-paste --watch cliphist store",
    "kanshi",
]
exec = []                             # run every reload
honor_xdg_autostart = true            # run ~/.config/autostart/*.desktop entries

[env]
# Variables set in the compositor's environment and inherited by all child processes.
# GriddyWM automatically sets WAYLAND_DISPLAY, DISPLAY (if XWayland), XDG_SESSION_TYPE,
# XDG_CURRENT_DESKTOP, XCURSOR_THEME, and XCURSOR_SIZE from the cursor config.
# Add app-compat vars here:
GTK_BACKEND               = "wayland"
QT_QPA_PLATFORM           = "wayland"
SDL_VIDEODRIVER           = "wayland"
ELECTRON_OZONE_PLATFORM_HINT = "auto"
MOZ_ENABLE_WAYLAND        = "1"
_JAVA_AWT_WM_NONREPARENTING = "1"
# Override or add any other env var:
# MY_VAR = "value"

[xwayland]
enable = true
hidpi = true
scale = 1.0
```

### Includes
`~/.config/griddy/` may also contain:
- `keybinds.toml` (§9)
- `rules.toml` (§10)
- `theme.toml` (§8.5 — appearance, borders, shadows, wallpaper)
- `monitors.toml` (§8.6 — per-output settings)
- `gestures.toml` (touch & touchpad binds; see §9.4)
- `workspace-templates.toml` (§8.10 — per-workspace layout presets)

All are included automatically unless explicitly disabled.

---

### 8.5 `theme.toml` — Appearance

Theming is split out so users can hot-swap visual styling (and tools like `pywal`, theme switchers, or shell-driven dark/light toggles can drop in a single file).

**Named presets.** GriddyWM ships built-in themes. Import one as the base and override individual tokens:

```toml
# ~/.config/griddy/theme.toml
import = "catppuccin-mocha"   # catppuccin-mocha | catppuccin-latte | tokyo-night | tokyo-night-light |
                               # nord | gruvbox-dark | gruvbox-light | solarized-dark | solarized-light |
                               # dracula | everforest-dark | rose-pine
# Any setting below overrides the imported preset:
[colors]
accent = "#ff9e64"             # override just the accent color, keep the rest
```

Built-in presets live in `/usr/share/griddy/themes/`. Users can add custom presets to `~/.config/griddy/themes/`.

```toml
# ~/.config/griddy/theme.toml

[wallpaper]
# GriddyWM does NOT ship a wallpaper daemon. It calls a configured tool
# via wlr-layer-shell background. Empty = no wallpaper (solid bg_color).
tool = "swaybg"                         # swaybg | swww | mpvpaper | hyprpaper | custom
mode = "fill"                           # fill | fit | stretch | center | tile
bg_color = "#1a1b26"                    # used if no image, and as letterbox color
# Per-monitor and per-workspace overrides supported:
default = "~/Pictures/wallpapers/grid.jpg"

[[wallpaper.per_monitor]]
monitor = "DP-1"
image   = "~/Pictures/wallpapers/ultrawide.jpg"

[[wallpaper.per_workspace]]
cell    = [0, 0]
image   = "~/Pictures/wallpapers/work.jpg"

[[wallpaper.per_workspace]]
cell    = [2, 2]
image   = "~/Pictures/wallpapers/play.jpg"

[colors]
# Named tokens referenced by everything below
accent          = "#7aa2f7"
accent_dim      = "#3d59a1"
warn            = "#e0af68"
danger          = "#f7768e"
ok              = "#9ece6a"
fg              = "#c0caf5"
fg_dim          = "#a9b1d6"
bg              = "#1a1b26"
bg_alt          = "#24283b"
border_idle     = "#414868"
shadow          = "#000000aa"

# ---------------------------------------------------------------
# Window decorations. Each window state has its own block so you
# can fully restyle focused / unfocused / urgent / floating, etc.
# ---------------------------------------------------------------

[window.focused]
border_px           = 2
border_color        = "{accent}"          # token expansion supported
border_gradient     = ["#7aa2f7", "#bb9af7"]    # if set, overrides border_color (linear)
border_gradient_angle = 45                # degrees, clockwise from top
rounded_corners_px  = 8
opacity             = 1.00
shadow              = true
shadow_color        = "{shadow}"
shadow_blur_px      = 24
shadow_offset       = [0, 6]
shadow_spread_px    = 0
inactive_dim        = 0.0                 # 0..1, dim non-content (only meaningful for focused = no)
title_bar           = "none"              # "none" | "thin" | "full"
title_bar_height_px = 0

[window.unfocused]
border_px           = 2
border_color        = "{border_idle}"
rounded_corners_px  = 8
opacity             = 0.97                # subtle transparency on unfocused
shadow              = true
shadow_color        = "{shadow}"
shadow_blur_px      = 14
shadow_offset       = [0, 3]
inactive_dim        = 0.08                # dim 8% on unfocused

[window.urgent]
border_px           = 3
border_color        = "{warn}"
border_pulse_ms     = 600                 # 0 = no pulse animation
opacity             = 1.0

[window.floating]
# Inherits from focused/unfocused; only overrides
border_px           = 2
rounded_corners_px  = 12                  # rounder by convention
shadow_blur_px      = 32                  # bigger shadow to lift it visually
shadow_offset       = [0, 10]

[window.fullscreen]
# When in Fullscreen mode (not TotalFullscreen)
border_px           = 0
rounded_corners_px  = 0
shadow              = false

[window.total_fullscreen]
# Pure passthrough — no decoration, no shaders, optional tearing
border_px           = 0
rounded_corners_px  = 0
shadow              = false
disable_blur        = true
disable_shaders     = false               # set true for max raw perf

[window.stacked]
# ---- Stack depth cue (the "deck of cards" peek behind the top window) ----
depth_cue               = true          # show back-edges of windows below top
depth_offset_px         = 4             # px each layer shifts right + down from the one above
depth_max_layers        = 3             # max layers to render depth cues for (regardless of stack size)
depth_shadow            = true          # subtle shadow between each visible layer

# ---- Stack indicator pill ----
indicator               = "pill"        # "pill" | "dots" | "tabs" | "none"
indicator_position      = "top-right"   # "top-left" | "top-right" | "bottom-left" | "bottom-right"
indicator_bg            = "{accent}"
indicator_fg            = "#ffffff"
indicator_font_size     = 11
indicator_padding_px    = 6             # horizontal padding inside the pill
indicator_radius_px     = 10
indicator_opacity_idle  = 0.65          # opacity when cursor is away from pill
indicator_opacity_hover = 1.0           # opacity on hover (cursor over pill)
indicator_color         = "{accent}"    # kept for backwards compat with "dots" | "tabs"

# ---- Stack-peek mode ----
peek_style              = "cascade"     # "cascade" | "fan" | "grid"
peek_cascade_offset_px  = 28            # px each window shifts right + down in cascade mode
peek_fan_angle_deg      = 7             # degrees between each window in fan mode
peek_dim_unstacked      = 0.40          # opacity of non-highlighted windows during peek
peek_highlight_border   = true          # accent border on highlighted window during peek

# ---------------------------------------------------------------
# Gaps
# Two independent gap systems:
#   - Window gaps: space between tiled windows within a workspace
#   - Workspace gaps: space between workspaces in overview mode,
#     and the visible seam between workspace edges during slides
# ---------------------------------------------------------------

[gaps.windows]
inner_px            = 8     # gap between adjacent tiled windows (on all shared edges)
outer_px            = 12    # gap between the outermost windows and the screen edge
# Per-edge outer gap overrides (take precedence over outer_px):
outer_top_px        = 12    # often larger to clear the bar
outer_bottom_px     = 12
outer_left_px       = 12
outer_right_px      = 12
# Smart gaps: collapse inner AND outer to 0 when only one window is in the workspace
smart               = true

[gaps.workspaces]
overview_gap_px     = 20    # gap between workspace thumbnails in overview mode
slide_gap_px        = 0     # gap (in logical px) visible between workspaces during a slide
                             # 0 = workspaces butt up directly (default, macOS-style)
                             # >0 = brief dark seam is visible between sliding workspaces

[blur]
enabled             = true
passes              = 2
size_px             = 8
noise               = 0.0117               # subtle noise hides banding
brightness          = 1.0
contrast            = 1.0
ignore_opacity      = false
# Blur only certain layer-shell namespaces (e.g. blur waybar bg but not bg image)
layer_namespaces    = ["waybar", "notifications"]

[dim]
# Global dim of inactive workspaces in overview, etc.
overview_unfocused  = 0.15
peek_unstacked      = 0.40

[cursor]
theme               = "Bibata-Modern-Classic"
size                = 24
inactivity_timeout_ms = 0                  # 0 = never hide; >0 hides after idle ms
hide_on_typing      = true

[font]
# Fallback for any built-in text (OSD, overview labels, error overlay)
family              = "Inter"
size                = 12
bold_focused        = true
```

Theme reload is a sub-operation of config reload; `griddyctl reload theme` reloads only `theme.toml` without re-evaluating keybinds (faster, no rebind churn).

---

### 8.6 `monitors.toml` — Per-output Configuration

```toml
[[monitor]]
name = "DP-1"                              # connector name from `griddyctl monitors`
mode = "2560x1440@165"                     # "preferred" | "WxH" | "WxH@Hz"
position = [0, 0]                          # logical pixel coords
scale = 1.0
transform = "normal"                       # normal|90|180|270|flipped|flipped-90|...
vrr = "fullscreen"                         # "off" | "on" | "fullscreen" (only enables in fullscreen)
adaptive_sync = true
bit_depth = 10                             # 8 | 10 (10 requires HDR-capable path)
color_profile = "auto"                     # auto | sRGB | rec2020 | path to .icc
hdr = false                                # opt-in (experimental)
tearing = true                             # allow tearing if window opts in
enabled = true

[[monitor]]
name = "HDMI-A-1"
mode = "preferred"
position = [2560, 0]
scale = 1.25
# This monitor uses its own grid:
grid_cols = 2
grid_rows = 2
# Pin this monitor — it never follows sync navigation (see §5 Multi-monitor workspace sync):
workspace_sync = false

[default]
# Applied to any monitor not matched above
mode      = "preferred"
scale     = 1.0
position  = "auto"                         # auto-arrange in a row
```

---

### 8.7 Submaps (modal keybinds)

Vim-style nested key modes. Enter with `submap <name>` (bound to a keybind). An OSD overlay (§8.9) shows the active submap name so users always know which mode they're in.

**Built-in: placement submap** (bound to `$mod+w` by default — see §9.1):

```toml
[[submap]]
name = "placement"
exit_keys = ["Escape"]
exit_on_unhandled = true   # any unrecognized key exits — prevents getting stuck

  [[submap.bind]]
  key = "f"
  action = "state-fullscreen-toggle"

  [[submap.bind]]
  key = "F"
  mods = ["Shift"]
  action = "state-total-fullscreen-toggle"

  [[submap.bind]]
  key = "h"
  action = "slot-half-left"

  [[submap.bind]]
  key = "l"
  action = "slot-half-right"

  [[submap.bind]]
  key = "u"
  action = "slot-quarter-tl"

  [[submap.bind]]
  key = "i"
  action = "slot-quarter-tr"

  [[submap.bind]]
  key = "j"
  action = "slot-quarter-bl"

  [[submap.bind]]
  key = "k"
  action = "slot-quarter-br"

  [[submap.bind]]
  key = "v"
  action = "state-floating-toggle"
```

**Built-in: resize submap** (bound to `$mod+r` by default — floating windows only):

```toml
[[submap]]
name = "resize"
exit_keys = ["Escape", "Return"]
exit_on_unhandled = false

  [[submap.bind]]
  key = "h"
  action = "resize-active -20 0"
  repeat = true

  [[submap.bind]]
  key = "l"
  action = "resize-active 20 0"
  repeat = true

  [[submap.bind]]
  key = "k"
  action = "resize-active 0 -20"
  repeat = true

  [[submap.bind]]
  key = "j"
  action = "resize-active 0 20"
  repeat = true
```

Custom submaps follow the same schema.

---

### 8.8 Idle & power

```toml
[idle]
# GriddyWM ships a built-in idle manager (no need for swayidle/hypridle, but they still work).
enabled = true

[[idle.timeout]]
after_seconds = 300
on_timeout    = "exec swaylock"
on_resume     = ""

[[idle.timeout]]
after_seconds = 600
on_timeout    = "dpms off"
on_resume     = "dpms on"

[idle.inhibitors]
# Honor idle_inhibit_unstable_v1 from apps (mpv, browsers in fullscreen video)
honor_app_inhibits = true
# Don't idle while any window has content-type = "game"
no_idle_on_game    = true
# Don't idle while audio is playing (requires pipewire)
no_idle_on_audio   = true
```

---

### 8.9 OSD (on-screen display)

GriddyWM ships a minimal built-in OSD overlay used for:
- Workspace switch indicator (small grid widget showing focused cell)
- Submap name display
- Config reload status (success / error — see §14.2)
- Volume / brightness when bound to `osd-show` dispatcher
- Stack peek preview

```toml
[osd]
enabled            = true
position           = "top-center"          # top-left|top-center|top-right|center|...
margin_px          = 24
duration_ms        = 1500
fade_ms            = 200
bg_color           = "{bg_alt}cc"          # alpha-suffixed hex ok
fg_color           = "{fg}"
font_family        = "Inter"
font_size          = 13
rounded_corners_px = 8
workspace_indicator = true
submap_indicator    = true
reload_indicator    = true
```

The OSD is **never** the primary notification system — it's for compositor-internal state only. Application notifications still need a notification daemon (§14.2).

---

### 8.10 `workspace-templates.toml` — Per-workspace layout presets

Define the initial tiled layout and startup apps for specific workspaces. Applied once on first visit to that workspace (not re-applied on reload).

```toml
# ~/.config/griddy/workspace-templates.toml

[[template]]
cell  = [0, 0]
name  = "work"

  [[template.window]]
  slot    = "HalfLeft"
  exec    = "kitty"
  app_id  = "^kitty$"          # if already running, move here instead of spawning

  [[template.window]]
  slot    = "HalfRight"
  exec    = "firefox"
  app_id  = "^firefox$"

[[template]]
cell = [2, 2]
name = "media"

  [[template.window]]
  slot  = "Fullscreen"
  exec  = "spotify"
  app_id = "^Spotify$"
```

Templates are applied lazily on first workspace visit, not at startup (avoids spawning every app at boot). Force-apply anytime with `griddyctl workspace apply-template <col,row>`.

---

### 8.11 Pointer constraints

Apps can lock the cursor via `pointer_constraints_v1` (games, remote desktop, drawing apps). Behavior:

- **Cursor lock:** app requests `zwp_locked_pointer_v1`. Compositor grants it. Cursor movement is reported as relative motion only; cursor doesn't move on screen.
- **Cursor confine:** app requests `zwp_confined_pointer_v1`. Cursor stays within the window's surface region.

**Escape hatch.** A hardcoded "constraint-break" bind (cannot be overridden or consumed by apps) ungrants any active pointer constraint and returns to normal cursor mode:

```toml
[input]
pointer_constraint_break_key = "Super+Escape"   # hardcoded-style; apps cannot intercept this
```

While a cursor is locked, workspace navigation keybinds (`$mod+Ctrl+hjkl`, overview, etc.) **still fire** — the app only captures pointer events, not compositor keybinds. Switching workspace auto-unconfines (but does not unlock — apps must re-request on focus regain).

---

### 8.12 Output hot-plug

```toml
[monitors]
primary = "DP-1"              # designated primary; fallback is first connected output

[monitors.on_disconnect]
policy    = "migrate-to-primary"   # migrate-to-primary | migrate-to-nearest | keep-in-grid
workspace = "focused"              # "focused" | "same-coords" — which workspace on the destination
                                   # "same-coords" tries (col,row) from the lost monitor; falls back to focused

[monitors.on_connect]
policy    = "apply-config"         # apply-config (monitors.toml) | mirror-primary | extend-right
workspace = "auto"                 # which workspace to show on the new output at first
```

**On disconnect:** all windows from the lost output's grid migrate to `primary` (or nearest if `migrate-to-nearest`). They land on the destination workspace per `workspace` policy. Their workspace coordinates are remembered — if the monitor reconnects, they move back automatically (`monitors.restore_on_reconnect = true`, default on).

**On connect:** `kanshi` (recommended, §Tier 2) handles output arrangement declaratively. GriddyWM fires a `monitor_added` IPC event and re-applies `monitors.toml` so kanshi or any listener can react.

---

## 9. Keybinds

### File: `keybinds.toml`

Two binding kinds: `[[bind]]` (regular) and `[[bind_release]]` (on key release; used for things like "hold to peek overview").

#### Syntax

```toml
[[bind]]
mods        = ["Super"]          # Super | Ctrl | Alt | Shift (any combination)
key         = "h"                # xkb keysym name
action      = "workspace-left"
args        = []                 # optional typed args
description = "Focus workspace to the left"
repeat      = false              # if true, action fires repeatedly while key is held
                                 # (uses keyboard repeat_rate / repeat_delay from [input.keyboard])
locked      = false              # if true, fires even when screen is locked
                                 # (use for media keys, brightness — hardware-level events)
global      = false              # if true, fires even when a TotalFullscreen window has input grab
                                 # (use for compositor-wide emergency binds like quit, screenshot)
passthrough = false              # if true, compositor runs the action AND forwards the keypress
                                 # to the focused app (useful for rebinding without shadowing)
```

**Flag guidance:**

| Use case | Recommended flags |
|---|---|
| Volume / brightness keys | `locked = true` |
| Hold to navigate workspaces | `repeat = true` |
| Quit / screenshot in a game | `global = true` |
| Composite shortcut (WM + app) | `passthrough = true` |
| Workspace peek (hold to show overview) | use `[[bind_release]]` instead |

#### Modifiers
`Super` (Mod4), `Alt` (Mod1), `Ctrl`, `Shift`, plus virtual `Mouse:Left/Right/Middle/Forward/Back` for mouse-button binds.

#### Pattern actions
Actions may take typed args. Both `action = "workspace"` with `args = [1,2]` and the shorthand `action = "workspace 1,2"` are accepted.

### 9.1 Default keybind table

`$mod` = `Super` unless `input.mod_key` is overridden.

| Mods | Key | Action | Notes |
|---|---|---|---|
| `$mod` | `Return` | `exec` `"kitty"` | Launch terminal (swap to `alacritty` if preferred) |
| `$mod` | `d` | `exec` `"fuzzel"` | App launcher |
| `$mod` | `q` | `close-window` | Close focused window |
| `$mod+Shift` | `e` | `quit` | Exit compositor |
| `$mod` | `r` | `submap resize` | Enter floating-window resize mode |
| `$mod+Shift` | `r` | `reload-config` | Force reload TOML |
| **Focus** | | | |
| `$mod` | `h` | `focus-left` | Focus window left |
| `$mod` | `j` | `focus-down` | Focus window down |
| `$mod` | `k` | `focus-up` | Focus window up |
| `$mod` | `l` | `focus-right` | Focus window right |
| `$mod` | `Tab` | `stack-next` | Next in current slot's stack |
| `$mod+Shift` | `Tab` | `stack-prev` | Prev in slot's stack |
| **Window state & slot** | | | |
| `$mod` | `f` | `state-fullscreen-toggle` | Toggle Fullscreen state |
| `$mod+Shift` | `f` | `state-total-fullscreen-toggle` | Toggle TotalFullscreen state |
| `$mod` | `v` | `state-floating-toggle` | Toggle Floating state |
| `$mod` | `Left` | `slot-half-left` | Assign HalfLeft slot |
| `$mod` | `Right` | `slot-half-right` | Assign HalfRight slot |
| `$mod` | `w` | `submap placement` | Enter placement submap (see below) |
| `$mod` | `c` | `center-floating` | Center current floating window |

**Placement submap** — entered with `$mod+w`, auto-exits after any selection:

| Key | Action | Slot |
|---|---|---|
| `f` | `state-fullscreen-toggle` | Fullscreen |
| `F` | `state-total-fullscreen-toggle` | TotalFullscreen |
| `h` | `slot-half-left` | HalfLeft |
| `l` | `slot-half-right` | HalfRight |
| `u` | `slot-quarter-tl` | ↖ QuarterTL |
| `i` | `slot-quarter-tr` | ↗ QuarterTR |
| `j` | `slot-quarter-bl` | ↙ QuarterBL |
| `k` | `slot-quarter-br` | ↘ QuarterBR |
| `v` | `state-floating-toggle` | Float |
| `Escape` | `submap reset` | Cancel |

The `u/i/j/k` positions mirror a loose keyboard-corner mnemonic (top row = top corners, bottom row = bottom corners). Numpad users can also bind `$mod+KP_7/9/1/3` directly in `keybinds.toml` — the numpad layout maps exactly to screen corners.
| **Workspace navigation (grid)** | | | |
| `$mod+Ctrl` | `h` | `workspace-left` | Slide to workspace left |
| `$mod+Ctrl` | `j` | `workspace-down` | Slide down |
| `$mod+Ctrl` | `k` | `workspace-up` | Slide up |
| `$mod+Ctrl` | `l` | `workspace-right` | Slide right |
| `$mod` | `1`..`9` | `workspace-index` | Linear index (row-major) |
| `$mod+Ctrl` | `o` | `workspace-back` | Previous workspace in history |
| `$mod+Ctrl` | `i` | `workspace-forward` | Forward in history |
| **Move window to workspace** | | | |
| `$mod+Shift+Ctrl` | `h` | `move-window-direction` `left` | |
| `$mod+Shift+Ctrl` | `j` | `move-window-direction` `down` | |
| `$mod+Shift+Ctrl` | `k` | `move-window-direction` `up` | |
| `$mod+Shift+Ctrl` | `l` | `move-window-direction` `right` | |
| `$mod+Shift` | `1`..`9` | `move-window-to-index` | |
| **Overview** | | | |
| `$mod` | `o` | `overview-toggle` | Enter/leave Overview |
| `$mod` | `grave` | `overview-toggle` | Backtick alias |
| (held) | `$mod` | `overview-peek` | Peek while held (release-bind) |
| In overview: `Enter` | | `overview-activate` | Enter focused workspace |
| In overview: arrows | | `overview-focus-direction` | Navigate thumbnails |
| In overview: `Tab` | | `overview-window-next` | Focus next window in thumbnail |
| In overview: `Shift+Tab` | | `overview-window-prev` | Focus prev window in thumbnail |
| In overview: `Space` | | `overview-grab-window` | Pick up window; arrows move it; Enter drops |
| **Diagonal workspace nav** | | | |
| `$mod+Ctrl` | `y` | `workspace-nw` | Diagonal: up-left |
| `$mod+Ctrl` | `u` | `workspace-ne` | Diagonal: up-right |
| `$mod+Ctrl` | `b` | `workspace-sw` | Diagonal: down-left |
| `$mod+Ctrl` | `n` | `workspace-se` | Diagonal: down-right |
| **Floating window manipulation (Mouse)** | | | |
| `$mod` | `Mouse:Left` (drag) | `move-window` | Drag-move floating window |
| `$mod` | `Mouse:Right` (drag) | `resize-window` | Drag-resize |
| **Stack** | | | |
| `$mod+Alt` | `Tab` | `stack-promote` | Top of stack |
| `$mod+Alt+Shift` | `Tab` | `stack-collapse` | Eject from stack |
| `$mod+Alt` | `Up` | `stack-move-up` | Reorder up in stack |
| `$mod+Alt` | `Down` | `stack-move-down` | Reorder down in stack |
| (held) | `$mod+grave` | `stack-peek` | Fan out stack while held |
| **Misc** | | | |
| (none) | `Escape` | `total-fullscreen-exit` | Only fires in TotalFullscreen |
| `$mod` | `s` | `screenshot` `region` | Region screenshot |
| `$mod+Shift` | `s` | `screenshot` `screen` | Whole screen |
| `$mod` | `p` | `cycle-shader` | Cycle screen shaders |
| `$mod` | `slash` | `cheatsheet-toggle` | Show all bound keys overlay |
| `$mod` | `comma` | `osd-show` `"griddy v0.1"` | Demo OSD |
| **Hardware keys** (`locked = true`) | | | |
| (none) | `XF86AudioRaiseVolume` | `exec "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%+"` | Volume up |
| (none) | `XF86AudioLowerVolume` | `exec "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-"` | Volume down |
| (none) | `XF86AudioMute` | `exec "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"` | Mute |
| (none) | `XF86AudioMicMute` | `exec "wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle"` | Mic mute |
| (none) | `XF86MonBrightnessUp` | `exec "brightnessctl set 5%+"` | Brightness up |
| (none) | `XF86MonBrightnessDown` | `exec "brightnessctl set 5%-"` | Brightness down |
| (none) | `XF86AudioPlay` | `exec "playerctl play-pause"` | Media play/pause |
| (none) | `XF86AudioNext` | `exec "playerctl next"` | Media next |
| (none) | `XF86AudioPrev` | `exec "playerctl previous"` | Media prev |
| (none) | `Print` | `screenshot region` | Region screenshot |
| `$mod+Shift` | `Escape` | `total-fullscreen-exit` | Break pointer constraint / exit TF (`global = true`) |

### 9.2 Action reference (alphabetical)

`center-floating`, `cheatsheet-toggle`, `close-window`, `cycle-shader`, `exec <command>`, `focus-up|down|left|right`, `minimap-toggle`, `move-to-special <name>`, `move-window <col,row>`, `move-window-direction <left|right|up|down>`, `move-window-to-index <n>`, `move-window-to-workspace-and-follow <col,row>`, `osd-show <message>`, `overview-activate`, `overview-focus-direction <dir>`, `overview-grab-window`, `overview-peek` (release-bind), `overview-toggle`, `overview-window-next`, `overview-window-prev`, `quit`, `reload-config`, `reload-theme`, `resize-active <dx> <dy>`, `screenshot <region|screen|window>`, `slot-half-left|right`, `slot-quarter-tl|tr|bl|br`, `spawn-floating <cmd>`, `spawn-in-slot <slot> <cmd>`, `spawn-on-workspace <col,row> <cmd>`, `spawn-stacked <cmd>`, `stack-collapse`, `stack-flip <n>`, `stack-move-down`, `stack-move-up`, `stack-next`, `stack-peek` (release-bind), `stack-prev`, `stack-promote`, `state-floating-toggle`, `state-fullscreen-toggle`, `state-total-fullscreen-toggle`, `submap <name>`, `toggle-special <name>`, `total-fullscreen-exit`, `workspace <col,row>`, `workspace-back`, `workspace-direction <left|right|up|down>`, `workspace-forward`, `workspace-index <n>`, `workspace-nw|ne|sw|se`, `workspace-rename <col,row> <name>`, `workspace-sync-toggle`.

**`resize-active <dx> <dy>` — Floating windows only.** Adjusts width and height by `dx`/`dy` pixels. If called on a Tiled, Fullscreen, or TotalFullscreen window, it does nothing and emits an OSD hint: *"resize only applies to floating windows — use `state-floating-toggle` first."* Custom split ratios for tiled slots are a v2 feature.

### 9.3 Mouse binds
`bind` accepts `key = "Mouse:Left"`, `Mouse:Right`, `Mouse:Middle`, `Mouse:Forward`, `Mouse:Back`, and `Scroll:Up`/`Scroll:Down`/`Scroll:Left`/`Scroll:Right`.

### 9.4 Touch / gesture binds

```toml
[[gesture]]
fingers = 4
direction = "up"          # up | down | left | right | pinch-in | pinch-out
action = "overview-toggle"

[[gesture]]
fingers = 3
direction = "left"
action = "workspace-right"   # natural-scroll-style inverse
```

---

## 10. Window Rules

Rules are evaluated in **declaration order, last rule wins**. If two rules match the same window and set the same property, the later rule in `rules.toml` takes precedence — identical to CSS cascade behavior. This means you put general rules first and specific overrides after.

```toml
# General: all kitty terminals get the CRT shader
[[rule]]
match.app_id = "^kitty$"
shader = "shaders/crt.glsl"

# Specific: the kitty instance on workspace (2,2) should NOT have the shader
# This comes AFTER the general rule, so it wins for workspace (2,2) kitty windows
[[rule]]
match.app_id = "^kitty$"
match.workspace = [2, 2]
shader = ""
```

When multiple rules match and set **different** properties (no conflict), all properties are merged — each rule contributes what it sets without cancelling the others.

```toml
[[rule]]
match.app_id = "^firefox$"
action = "workspace 0,0"

[[rule]]
match.title = "Picture-in-Picture"
action = "state floating"
floating_geom = [1280, 32, 480, 270]   # x, y, w, h
sticky = true

[[rule]]
match.app_id = "^mpv$"
match.fullscreen_request = true
action = "state total-fullscreen"
allow_tearing = true

[[rule]]
match.app_id = "^Steam$"
match.title_regex = "Friends List"
action = "state floating"
opacity = 0.97

[[rule]]
match.app_id = "^kitty$"
shader = "shaders/crt.glsl"
```

**Match fields:** `app_id`, `title`, `title_regex`, `pid`, `workspace`, `fullscreen_request`, `is_xwayland`.

**Actions / properties:** `action` (any dispatcher), `floating_geom`, `sticky`, `opacity`, `shader`, `border_color`, `decorations` (`"server"|"client"|"none"`), `no_animations`, `no_shadow`, `no_blur`, `min_size_overflow`, `above_total_fullscreen`, `swallow`.

---

## 11. Shader Pipeline

Shaders are GLSL ES 3.00 fragment programs compiled at load. Each category has its own uniform contract. Failed compilation falls back to a no-op shader and emits an IPC `shader_error` event with the compiler log.

### 11.1 Pipeline order (per frame)

```
For each output:
  1. Render background layer (wlr-layer-shell background)
  2. Render bottom layer (wlr-layer-shell bottom)
  3. For each tiled window (bottom→top in slot Z-order):
       a. Render client surface to per-window FBO
       b. Apply per-window shader (if any)
       c. Apply animation shader for in-progress animation (if any)
       d. Composite onto output FBO with shadow + rounded corners
  4. Render floating windows where above_total_fullscreen = false
       (above tiled, below shell — the normal floating layer)
  5. Render top layer (wlr-layer-shell top)
  6. Render Fullscreen stack top (if any) — covers layers 1–5
  7. Render overlay layer (wlr-layer-shell overlay)
  8. Render TotalFullscreen stack top (if any) — covers layers 1–7
  9. Render floating windows where above_total_fullscreen = true (same workspace only)
       — PiP, sticky terminals, drag previews that stay visible over fullscreen content
 10. Apply screen shader to output FBO (covers layers 1–9)
 11. Hardware cursor plane (DRM overlay; not in FBO and not affected by screen shader)
       — software cursor fallback renders here if hardware plane unavailable
 12. Present (with explicit sync)
```

This order means:
- Most floating windows sit between tiled content and the shell bar (step 4) — normal behavior.
- TotalFullscreen covers shell layers and fullscreen promotions (step 8) — correct.
- `above_total_fullscreen = true` floating windows (step 9) are the only things above TotalFullscreen besides the cursor, matching §6.4.
- The screen post-process shader (step 10) applies to all rendered content uniformly.

### 11.2 Per-event animation shaders

Each animation event has a dedicated shader hook. Built-ins live in `/usr/share/griddy/shaders/` and can be replaced per-event in `[shaders]`.

| Event | Hook | Built-in default | Trigger |
|---|---|---|---|
| Open | `open.glsl` | `scale-fade-in` | Window mapped |
| Close | `close.glsl` | `scale-fade-out` | Window unmapped |
| Move | `move.glsl` | `linear-translate` | Slot change in same workspace |
| Resize | `resize.glsl` | `geometry-lerp` | Mode change or floating resize |
| Workspace slide | `workspace_slide.glsl` | `slide-translate` | Focus change between workspaces |
| Overview zoom | `overview_zoom.glsl` | `radial-zoom` | Entering/leaving overview |
| Focus | `focus.glsl` | `border-pulse` | Window gains focus |
| Stack cycle | `stack.glsl` | `cross-fade` | `stack-next`/`stack-prev` |

### 11.3 Uniform contract

All animation shaders receive:

```glsl
#version 300 es
precision highp float;

// Standard inputs - always populated by GriddyWM
uniform sampler2D u_texPrev;      // previous frame / outgoing texture
uniform sampler2D u_texNext;      // incoming texture (may equal u_texPrev for single-source effects)
uniform float     u_progress;     // 0.0 -> 1.0 animation progress (post-easing)
uniform float     u_progressRaw;  // 0.0 -> 1.0 linear progress
uniform float     u_time;         // seconds since shader started
uniform float     u_timeAbs;      // seconds since compositor start
uniform vec2      u_resolution;   // output px
uniform vec2      u_windowPos;    // window top-left in output px (or 0,0 for fullscreen events)
uniform vec2      u_windowSize;   // window size in px
uniform vec2      u_cursorPos;    // cursor px
uniform vec2      u_direction;    // normalized slide direction vector.
                                  // Orthogonal: (-1,0)=left, (1,0)=right, (0,-1)=up, (0,1)=down.
                                  // Diagonal: e.g. (-0.707,-0.707)=NW, (0.707,-0.707)=NE.
                                  // (0,0) for non-slide animations.
                                  // NOTE: replaces the former int u_direction; update existing shaders.
uniform float     u_opacity;      // window opacity

in  vec2 v_uv;                    // 0..1 across the window FBO
out vec4 fragColor;

void main() {
    // animation logic here
}
```

User-defined per-window shaders receive an extra `void windowShader(inout vec4 color)` entry point convention (Hyprland-compatible style) so existing community shaders port with minimal change.

Custom uniforms supported via TOML:

```toml
[[shaders.custom_uniforms]]
shader = "shaders/crt.glsl"
name = "u_curvature"
type = "float"          # float|vec2|vec3|vec4|int|sampler2D
value = 0.05
```

### 11.4 Per-window shaders

Like Hyprland's `Hypr-DarkWindow`, GriddyWM supports per-window fragment shaders applied after the surface texture but before composite. Set via:

- TOML rule `shader = "path.glsl"`
- IPC dispatcher `set-window-shader <id> <path>`
- `griddyctl shader set --window <id> <path>`

### 11.5 Screen shader (post-process)

A single full-screen shader applied to the final output FBO, like Hyprland's `decoration:screen_shader`. Use cases: CRT, blue-light filter, color blindness simulators, vibrance. Configurable schedule:

```toml
[[shaders.screen_schedule]]
shader = "shaders/blue-light.glsl"
start = "19:00"
end = "06:00"

[[shaders.screen_schedule]]
shader = "shaders/vibrance.glsl"
default = true
```

### 11.6 Built-in animation shader examples

`scale-fade-in` (open):
```glsl
void main() {
    float s = mix(0.85, 1.0, u_progress);
    vec2 uv = (v_uv - 0.5) / s + 0.5;
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        fragColor = vec4(0.0);
    } else {
        vec4 c = texture(u_texNext, uv);
        c.a *= u_progress;
        fragColor = c;
    }
}
```

`slide-translate` (workspace_slide):
```glsl
void main() {
    // u_direction is a normalized vec2 — works for orthogonal AND diagonal slides
    vec2 uvPrev = v_uv + u_direction * u_progress;
    vec2 uvNext = v_uv + u_direction * (u_progress - 1.0);
    vec4 a = texture(u_texPrev, uvPrev);
    vec4 b = texture(u_texNext, uvNext);
    // Choose which texture to show based on which side of the transition we're on
    float blend = dot(uvNext - 0.5, u_direction) > 0.0 ? 1.0 : 0.0;
    fragColor = mix(a, b, blend);
}
```

### 11.7 Performance guardrails

- Per-window FBOs are only allocated for windows that actually need them (animating, opacity != 1.0, has per-window shader, or has rounded corners + shadow). Static windows render direct-to-output (Smithay's damage-tracked path).
- Shader programs cached; recompile only on file change.
- Overview thumbnails reuse the same FBO pool sized to thumbnail dimensions.
- Tearing path skips shaders entirely for `TotalFullscreen` + `allow_tearing` + `content-type = game`.

### 11.8 Animation cancellation

When a new animation starts on a window that is already mid-animation, the compositor must decide what to do with the in-progress animation. Three policies:

| Policy | Behavior |
|---|---|
| `snap-then-start` (**default**) | Snap the in-progress animation to its end state immediately, then start the new animation from the end position. Prevents visual discontinuities; feels snappy. |
| `crossfade` | Cross-fade from the current mid-animation frame to the new animation's start frame over `animations.crossfade_ms`. Smoother but adds visual complexity and a brief latency. |
| `immediate` | Abandon in-progress animation, start new one from the window's *actual* current position (not end state). Can look jumpy if the window was mid-slide. |

```toml
[animations]
on_interrupt = "snap-then-start"   # snap-then-start | crossfade | immediate
crossfade_ms = 60                  # only used when on_interrupt = "crossfade"
```

All three policies apply uniformly to every animation category (open, close, move, workspace_slide, etc.). Per-category overrides are not supported — it complicates the animation state machine for minimal gain.

---

## 12. IPC

Two UNIX-domain sockets per running instance, located under `$XDG_RUNTIME_DIR/griddy/$GRIDDY_INSTANCE_SIGNATURE/`:

- `.command.sock` — request/response (synchronous).
- `.events.sock` — push events (subscribe by opening).

The signature is exported as `$GRIDDY_INSTANCE_SIGNATURE` to every child process (similar to `$HYPRLAND_INSTANCE_SIGNATURE`).

### 12.1 Command socket

#### Request framing
Plain UTF-8. Each request is a single line terminated by `\n`. Use prefix `j/` to request JSON response, otherwise human-readable text:

```
dispatch workspace 1,2
j/workspaces
j/dispatch workspace 1,2
[[BATCH]] dispatch slot-half-left ; dispatch focus-right
```

`[[BATCH]]` runs commands sequentially in one connection.

#### Top-level commands
| Command | Returns | Description |
|---|---|---|
| `dispatch <action> [args]` | `ok` / error text | Run a dispatcher (same names as in keybinds) |
| `j/dispatch <action> [args]` | JSON `{"ok":true}` or `{"ok":false,"error":"..."}` | |
| `keyword <key> <value>` | `ok` | Runtime config override |
| `j/keyword` | JSON | |
| `reload` | `ok` | Re-read config |
| `kill` | `ok` | Graceful shutdown |
| `version` | text | |
| `j/version` | `{"version":"...","commit":"...","branch":"..."}` | |
| `monitors` / `j/monitors` | list | All `wl_output`s |
| `workspaces` / `j/workspaces` | list | All grid cells |
| `windows` / `j/windows` | list | All windows |
| `activeworkspace` / `j/activeworkspace` | obj | |
| `activewindow` / `j/activewindow` | obj | |
| `layers` / `j/layers` | list | Layer-shell surfaces |
| `animations` / `j/animations` | obj | Current curves |
| `shaders` / `j/shaders` | list | Loaded shaders |
| `grid` / `j/grid` | obj | Grid dimensions, focused cell |
| `getoption <name>` / `j/getoption <name>` | value | |
| `notify <icon> <duration_ms> <msg>` | `ok` | Pop notification via shell |
| `setprop <window> <prop> <value>` | `ok` | Per-window prop |
| `cursorpos` / `j/cursorpos` | `{x, y}` | |
| `globalshortcuts` / `j/globalshortcuts` | list | xdg-desktop-portal binds |

#### Sample JSON responses

`j/grid`:
```json
{
  "cols": 3,
  "rows": 3,
  "scope": "per-output",
  "wrap_x": false,
  "wrap_y": false,
  "active": {"col": 1, "row": 1, "monitor": "DP-1"}
}
```

`j/workspaces`:
```json
[
  {
    "id": "DP-1:1,1",
    "col": 1, "row": 1,
    "monitor": "DP-1",
    "name": "default",
    "windows": 3,
    "focused": true
  }
]
```

`j/activewindow`:
```json
{
  "id": 7,
  "app_id": "mpv",
  "title": "movie.mkv",
  "workspace": {"col": 0, "row": 0, "monitor": "DP-1"},
  "current_state": "Tiled",
  "requested_state": "Fullscreen",
  "current_slot": "HalfRight",
  "requested_slot": null,
  "stack_index": 0,
  "stack_kind": "tiled-slot",
  "geometry": [1280, 0, 1280, 1440],
  "pid": 18234,
  "xwayland": false,
  "shader": null,
  "opacity": 1.0
}
```
This window asked for `Fullscreen`, but the workspace had something in HalfLeft, so it adapted to `HalfRight` (§6.5.1 cascade). When the HalfLeft window leaves, this one will auto-promote back to Fullscreen (`fullscreen_auto_restore = true`).

### 12.2 Event socket

Line-delimited push events:

```
<eventname>>><data>\n
```

(Two `>>` chosen to avoid colliding with `>>` redirections in app titles; final newline.)

#### Event catalogue

| Event | Data |
|---|---|
| `workspace_changed` | `col,row,monitor` |
| `workspace_created` | `col,row,monitor,name` |
| `workspace_renamed` | `col,row,monitor,old,new` |
| `workspace_sync_changed` | `mode` (`synced` \| `unsynced`) |
| `window_opened` | `id,app_id,workspace_col,workspace_row,state,slot` |
| `window_closed` | `id` |
| `window_moved` | `id,col,row` |
| `window_placed` | `id,policy_matched,slot_assigned` |
| `window_state_changed` | `id,state` |
| `window_slot_changed` | `id,slot` |
| `window_slot_adapted` | `id,requested,actual` |
| `window_state_adapted` | `id,requested_state,actual_state,actual_slot` |
| `window_move_skipped` | `id,requested_col,requested_row,actual_col,actual_row` |
| `window_move_refused` | `id,requested_col,requested_row,reason` |
| `window_size_constraint_forced_float` | `id,slot,min_w,min_h,slot_w,slot_h` |
| `window_focus` | `id` |
| `window_title` | `id,title` |
| `window_app_id` | `id,app_id` |
| `window_stack_changed` | `slot,top_id,size` |
| `window_stack_reordered` | `slot,order_csv` |
| `monitor_added` | `name,width,height,scale` |
| `monitor_removed` | `name` |
| `monitor_config_changed` | `name` |
| `overview_opened` | (empty) |
| `overview_closed` | (empty) |
| `view_mode_changed` | `focus|overview` |
| `submap_changed` | `name` |
| `keyboard_layout_changed` | `device,layout` |
| `config_reloaded` | `which` (= `all`, `theme`, `keybinds`, `rules`, `monitors`) |
| `config_error` | `path:line:col message` |
| `theme_reloaded` | (empty) |
| `shader_loaded` | `category,path` |
| `shader_error` | `category,path,log` |
| `notification_daemon_missing` | (empty) |
| `urgent` | `id` |
| `idle` | (empty) |
| `resume` | (empty) |
| `lock` | (empty) |
| `unlock` | (empty) |
| `safe_mode_entered` | `reason` |

### 12.3 `griddyctl` CLI

Wraps the command socket. Mirrors `hyprctl` UX.

```
griddyctl version
griddyctl dispatch workspace 1,2
griddyctl -j workspaces
griddyctl keyword animations.enabled false
griddyctl reload
griddyctl monitors
griddyctl --batch "dispatch slot-half-left ; dispatch focus-right"
griddyctl shader set --window 7 ~/shaders/crt.glsl
griddyctl shader screen ~/shaders/blue-light.glsl
griddyctl grid resize 4 4
```

### 12.4 Language bindings (planned)
Provided as separate crates / packages: `griddy-rs` (Rust), `griddy-py` (Python), `griddy-go` (Go), `griddy-ts` (Node). All wrap the two sockets, expose typed dispatchers, and emit typed events.

---

## 13. Plugin System

Out-of-tree plugins as dynamically loaded `cdylib`s through a stable C ABI.

```toml
# plugins.toml
[[plugin]]
path = "/usr/lib/griddy/plugins/libtouchpad-tweaks.so"
enabled = true

[plugin.touchpad-tweaks]
some_setting = 42
```

### Plugin ABI surface
- Register custom dispatchers (callable from keybinds & IPC).
- Register custom rules-matchers.
- Register custom animation shaders by name.
- Hook lifecycle events (`pre_render`, `post_render`, `window_pre_open`, `window_post_close`, `frame_begin`, `frame_end`).
- Allocate per-window state via opaque handle (no direct compositor pointer).

Version negotiation on load; mismatched ABI plugins refuse to load with a `plugin_error` IPC event.

---

## 14. Compatibility

### Display managers
Ship `/usr/share/wayland-sessions/griddy.desktop`:

```ini
[Desktop Entry]
Name=GriddyWM
Comment=Grid-based Wayland compositor
Exec=griddy
Type=Application
DesktopNames=GriddyWM
```

Compatible with **LightDM** (with `lightdm-gtk-greeter` ≥ 2.0.8), **SDDM** (Wayland greeter), **GDM** (forces own session by default; works as user-selectable session), **greetd** / **tuigreet**, **ly**.

### Shells / bars / launchers
- **WayBar** — works via `wlr-foreign-toplevel-management` + `ext-workspace-v1`. Ships a sample `waybar.config` snippet that uses `custom/griddy-workspaces` script piping `griddyctl -j workspaces` for the grid pill UI.
- **Noctalia / DMS / Quickshell-based shells** — work via the same `ext-workspace-v1` and the GriddyWM IPC (sample QML modules included).
- **Fuzzel / Wofi / Rofi-Wayland / Anyrun / Walker** — any `wlr-layer-shell` launcher works.
- **swaybg / swww / mpvpaper / hyprpaper** — wallpapers via `wlr-layer-shell` background (configured in `theme.toml` §8.5).
- **swayidle / hypridle** — idle management via standard idle-notify protocols (built-in alternative ships in `griddy idle`; see §8.8).
- **xdg-desktop-portal-gtk / -gnome** + GriddyWM-specific portal (planned): screen capture, file picker, global shortcuts, settings.

### 14.2 Notifications

GriddyWM does not ship its own full notification daemon — it integrates with the existing Wayland notification ecosystem (Mako, Dunst, Fnott, SwayNC, etc., all of which use `wlr-layer-shell`). However:

- The compositor **requires** a `org.freedesktop.Notifications` D-Bus name to be reachable at startup for application notifications to reach the user.
- If no daemon is detected within 3 seconds of the first notification request, the compositor logs a `notification_daemon_missing` event on the IPC bus and surfaces a one-shot OSD warning (the same overlay system from §8.9). Failing to do this would let bad reloads and other critical events go silent.
- The built-in **OSD overlay** (§8.9) is independent of D-Bus and is always available for compositor-internal messages, including:

#### Reload notifications (required behavior)
On any config reload (file change, `SIGUSR1`, or `griddyctl reload`):

| Outcome | OSD action | IPC event | D-Bus notification |
|---|---|---|---|
| Success, no changes | none (silent) | `config_reloaded` | none |
| Success, changes applied | green check, "Config reloaded" (1.2s) | `config_reloaded` | optional, off by default |
| Parse error | red overlay, file:line:col + error message, **persists 8s or until dismissed with `Escape`** | `config_error` | also sent as critical urgency if daemon present |
| Apply error (parsed but couldn't apply, e.g. shader compile failure) | yellow overlay, message + path, persists 5s | `config_error` | sent as normal urgency |

The OSD overlay is independent of any notification daemon, so a broken `theme.toml` or shader still produces visible feedback even with no notification daemon running. This guarantees users are never blind to a bad reload.

Configurable in `theme.toml` `[osd]` block (§8.9).

#### Recommended daemons
- **Mako** — minimal, well-tested.
- **SwayNC** — adds notification center / history pane.
- **Dunst** — works in Wayland mode since v1.9.

Sample `mako` config ships in `/usr/share/griddy/contrib/mako.conf`.

### XWayland
Optional. Enables apps like Steam, Discord, JetBrains IDEs, Wine/Proton. Surfaces participate in slots and states like native Wayland clients. HiDPI handled per `xwayland.scale`.

---

## 15. Build, Packaging, Install

### Build
```bash
# Arch
sudo pacman -S rustup wayland wayland-protocols libxkbcommon mesa libinput \
               libseat systemd-libs cairo pango udev pkgconf

git clone https://github.com/<org>/griddywm
cd griddywm
cargo build --release --features "xwayland,plugin-abi,vulkan"
sudo install -Dm755 target/release/griddy        /usr/bin/griddy
sudo install -Dm755 target/release/griddyctl     /usr/bin/griddyctl
sudo install -Dm644 dist/griddy.desktop          /usr/share/wayland-sessions/griddy.desktop
sudo install -Dm644 dist/portal.conf             /usr/share/xdg-desktop-portal/griddy-portals.conf
sudo install -d /usr/share/griddy/shaders
sudo install -m644 shaders/*.glsl                /usr/share/griddy/shaders/
```

### Feature flags
- `xwayland` (default)
- `plugin-abi` (default)
- `vulkan` (off; alternate renderer)
- `systemd` (default; `loginctl` session integration)
- `elogind` (alt to systemd)

### CLI flags

| Flag | Behavior |
|---|---|
| `griddy` | Start compositor normally |
| `griddy --check` | Parse all config files, print errors/warnings, exit 0 (valid) or 1 (invalid). Never starts the compositor. Use in dotfiles CI. |
| `griddy --replace` | Start compositor and gracefully replace the currently running instance. Sends `SIGUSR2` to the existing instance, which hand-offs DRM master before exiting. |
| `griddy --config <path>` | Override config file location |
| `griddy --gpu <device>` | Pin primary render GPU (e.g. `/dev/dri/card1`) |
| `griddy --no-xwayland` | Disable XWayland for this session regardless of config |
| `griddy -d` | Debug mode: `RUST_LOG=debug`, enables damage overlay and FPS counter |

### Distribution targets (v1)
Arch (AUR), Fedora (Copr), Nix flake, NixOS module, Debian/Ubuntu (PPA), openSUSE OBS, FreeBSD port.

---

## 16. Security & Sandboxing

- `security-context-v1` enforced: sandboxed clients can be denied access to `wlr-foreign-toplevel-management`, screencopy, virtual-keyboard, etc.
- IPC sockets are `0700` and bound to the user's `XDG_RUNTIME_DIR`. No TCP. No abstract sockets.
- Per-window screencopy gating: configurable allowlist (`screencopy.allow = ["org.freedesktop.impl.portal.desktop.*"]`).
- Global shortcuts go through `xdg-desktop-portal` for sandboxed apps.

---

## 17. Performance Targets

| Metric | Target |
|---|---|
| Idle CPU (60Hz, no animations) | < 1% on Ryzen 5 5600 |
| Workspace slide @ 240Hz | sustained 240 fps with all default shaders |
| Overview @ 4K | ≥ 60 fps with 9 workspaces, 30+ windows |
| Cold start to first frame | < 250 ms |
| Config reload | < 50 ms |
| IPC command roundtrip | < 1 ms p99 (local socket) |
| Memory (3x3 grid, 20 windows) | < 200 MB RSS |

---

## 18. Testing

- **Unit:** layout solver, grid math, config parser, rules matcher, IPC framing.
- **Integration:** `griddy --headless` backend (no DRM, no real output) drives scripted scenarios; assertions over `j/` IPC.
- **Visual regression:** headless backend renders to PNG; pixel-diff against golden masters per shader / animation frame.
- **Stress:** `griddy-bench` opens N windows, cycles modes, slides between workspaces; reports frame-time histograms.
- **Conformance:** `wayland-protocols` test suite, plus internal protocol fuzzing for `xdg_shell` and `wlr_layer_shell`.

---

## 19. Roadmap

### Phase 0 — Foundation (weeks 1-4)
- [ ] Smithay scaffolding, winit backend for dev
- [ ] DRM/KMS backend
- [ ] `xdg_shell`, `wl_seat`, libinput input pipeline
- [ ] Logging, panic handler, crash recovery
- [ ] `[env]` block: set WAYLAND_DISPLAY, XDG_SESSION_TYPE, GTK_BACKEND, etc. at startup

### Phase 1 — Grid, Slots, States (weeks 5-10)
- [ ] Grid data model (NxN, per-output + global modes)
- [ ] Tiled slots (Halves, Quarters) + slot-coexistence matrix
- [ ] Window states (Tiled, Floating, Fullscreen, TotalFullscreen) + promotion stacks
- [ ] Intra-slot & inter-slot conflict resolver + slot adaptation (§6.5)
- [ ] New window placement policy (§6.9): first window → Fullscreen, transient → float
- [ ] Focus-on-close policy (§6.10)
- [ ] Focus navigation: spatial adjacency + cross_workspace_focus (§6.11)
- [ ] Stacking (stack-next, stack-prev, stack-promote, stack-collapse)
- [ ] Z-order correct pipeline (tiled → floating-below-TF → top-layer → Fullscreen → TF → floating-above-TF)
- [ ] Gaps: window inner/outer gaps (§8.5 [gaps.windows])
- [ ] Basic damage tracking
- [ ] No animations yet — straight cuts

### Phase 2 — Config & Keybinds (weeks 11-13)
- [ ] TOML loader with hot reload, multi-file imports
- [ ] theme.toml: borders, shadows, rounded corners, gaps, blur config
- [ ] monitors.toml: per-output resolution/scale/position/VRR
- [ ] Keybind dispatcher with repeat, locked, global, passthrough flags
- [ ] Placement submap + resize submap (§8.7)
- [ ] Mouse / gesture binds
- [ ] Submaps (modal keybind modes)
- [ ] Rules engine: last-wins cascade, match fields, property merge (§10)
- [ ] Window size constraint handling: min_size → float policy (§6.13)
- [ ] `griddy --check` config validation

### Phase 3 — IPC (weeks 14-16)
- [ ] Command socket + event socket (dual UNIX sockets)
- [ ] `griddyctl` CLI (j/ JSON, --batch, all commands)
- [ ] Full event catalogue (§12.2)
- [ ] `griddy --replace` live replacement

### Phase 4 — Renderer & Shaders (weeks 17-22)
- [ ] Per-window FBOs, shadow, rounded corners, borders
- [ ] Blur pipeline (§8.5 [blur])
- [ ] Animation system (curves, springs, beziers)
- [ ] Animation cancellation: snap-then-start policy (§11.8)
- [ ] Per-event shader hooks + vec2 u_direction uniform (§11.2, §11.3)
- [ ] Per-window shader support (§11.4)
- [ ] Screen shader + schedule (§11.5)
- [ ] Gaps: workspace slide gap rendering (§8.5 [gaps.workspaces].slide_gap_px)
- [ ] Edge snap: drag floating → slot assignment (§6.12)
- [ ] TotalFullscreen: tearing path, floating-above-TF rendering

### Phase 5 — Overview & Navigation (weeks 23-28)
- [ ] Thumbnail FBO pool
- [ ] Overview transition shader (radial zoom)
- [ ] Mouse: click workspace, click window, drag window between workspaces
- [ ] Keyboard: arrow nav, Tab/Shift+Tab window cycle, Space grab-and-move
- [ ] Stack badge expand + drag reorder in overview
- [ ] Hot corner, touchpad swipe (4-finger up)
- [ ] Gaps: overview_gap_px between thumbnails (§8.5 [gaps.workspaces])
- [ ] Diagonal workspace navigation: vec2 direction, smooth single-motion slide (§5, §7.1)
- [ ] Minimap HUD for ≥4×4 grids (§7.4)
- [ ] Workspace switch OSD indicator

### Phase 6 — Shell / Portal / System (weeks 29-34)
- [ ] `ext-workspace-v1`, `wlr-foreign-toplevel-management`, `wlr-output-management-unstable-v1`
- [ ] `wlr-layer-shell`, `wlr-screencopy`
- [ ] `xdg-desktop-portal-griddy`
- [ ] XWayland integration
- [ ] Notification daemon detection + OSD fallback (§14.2)
- [ ] OSD system: reload status, submap indicator, workspace indicator (§8.9)
- [ ] Idle management built-in + DPMS (§8.8)
- [ ] Screen lock via `ext-session-lock-v1`
- [ ] Pointer constraint handling + break key (§8.11)
- [ ] Output hot-plug: migrate windows, restore on reconnect (§8.12)
- [ ] Cursor theme/size from config, exported via XCURSOR env vars
- [ ] XDG autostart honor (§8 startup)

### Phase 7 — Polish, Templates, Plugin ABI, 1.0 (weeks 35-42)
- [ ] Named theme presets: catppuccin, tokyo-night, nord, gruvbox, etc. (§8.5)
- [ ] Workspace templates: per-workspace initial layout + auto-exec (§8.10)
- [ ] Workspace-templates lazy-apply on first visit
- [ ] Window swallowing (§22.4)
- [ ] Special / scratchpad workspaces (§22.3)
- [ ] Mouse warping + focus-stealing prevention (§22.5)
- [ ] Window urgency via xdg_activation_v1 (§22.6)
- [ ] Migration tooling: import Hyprland/Niri/Sway configs (§22.15)
- [ ] Plugin C ABI freeze
- [ ] First-run experience: copy default config, show cheatsheet OSD
- [ ] `griddyctl cheatsheet` overlay
- [ ] Docs site
- [ ] Conformance + visual regression suite green on CI matrix

---

## 20. Open Questions

Remaining design decisions not yet locked in:

- **Default mod key:** `Super` (current) vs `Alt`. Super avoids conflicts with X11-era app bindings; Alt is more accessible on some keyboards. **Leaning Super.**
- **Linear workspace index ordering:** row-major (current) vs column-major. Row-major matches reading order. **Staying row-major.**
- **TotalFullscreen cursor hiding:** hide cursor by default when in TotalFullscreen? Games want it visible (they manage it themselves); video players don't care. **Current: do not hide — let the TF app control its own cursor.**
- **Per-output vs global grid as default:** per-output (current) is more flexible but harder to explain. **Staying per-output; `global` is one config line away.**
- **Bind syntax:** `[[bind]]` arrays (current) support all metadata fields (`repeat`, `locked`, `global`, `passthrough`, `description`). Inline `super+h = "workspace-left"` is cleaner for simple cases but can't express the full flag set. **Keeping `[[bind]]` arrays; the flag table makes them readable.**

---

## 21. Glossary

- **Grid** — the NxN array of workspaces.
- **Workspace** — a single `(col, row)` cell holding a set of windows.
- **Tiled slot** — one of the six geometric regions (HalfLeft/Right, QuarterTL/TR/BL/BR) within a workspace.
- **Window state** — `Tiled`, `Floating`, `Fullscreen`, or `TotalFullscreen`. Orthogonal to slot.
- **Promotion** — entering `Fullscreen` or `TotalFullscreen` state; the underlying tiled slot is remembered, not vacated.
- **Adaptation** — automatic slot/state reassignment when the requested target is occupied but a compatible alternative is available (§6.5.1).
- **Stack** — multiple windows occupying the same slot or promotion target. Top renders; rest are hidden.
- **Focus mode** — single workspace shown at 1:1 scale.
- **Overview mode** — zoomed-out view of all workspaces.
- **Minimap** — persistent corner HUD showing the full grid for orientation (§7.4).
- **Edge snap** — dragging a floating window to a screen edge to auto-assign it to the adjacent tiled slot (§6.12).
- **Dispatcher** — a named action invokable by keybind or IPC.
- **GIS** — GriddyWM Instance Signature, environment variable scoping IPC sockets.
- **OSD** — On-Screen Display, built-in overlay for compositor-internal messages.
- **Submap** — a named keybind mode (§8.7); active submaps replace the normal bind table until exited.
- **Scratchpad** — a special off-grid workspace for ephemeral windows (§22.3).

---

## 22. Things Still to Address

Things the spec doesn't yet cover in depth that should land before 1.0. Each is a known gap, not an unknown unknown.

### 22.1 Session state & persistence
- Save grid+window assignments to `$XDG_STATE_HOME/griddy/session.json` on graceful shutdown and every N seconds.
- On restart, optionally restore window→workspace assignments by `app_id`+`title` regex (best-effort, since PIDs change).
- `griddyctl session save/restore/clear`.

### 22.2 Crash recovery / safe mode
- If `griddy` crashes 3 times within 30 seconds, the next launch enters **safe mode**: ignores `config.toml`, loads `safe.toml` (minimal hardcoded keybinds: `$mod+q`, `$mod+Return`, `$mod+Shift+e`), disables all shaders and animations.
- OSD shows "SAFE MODE — config disabled" until user fixes config and runs `griddyctl exit-safe-mode`.

### 22.3 Special / scratchpad workspaces
The NxN grid is finite, but users still want ephemeral scratchpads (drop-down terminal, password manager, etc.).
- Reserved "special" workspaces named `special:<name>`, not on the grid.
- Dispatchers: `toggle-special <name>` (slide in/out from edge), `move-to-special <name>`.
- Configurable slide direction, size (% of screen), and animation per scratchpad.

### 22.4 Window swallowing
Terminals that spawn GUI apps (e.g. launching `imv image.png` or `mpv video.mkv` from `kitty`) should optionally "swallow" the terminal — the GUI takes the terminal's slot, terminal hides until GUI closes.
- Configurable per-rule: `swallow = "^kitty$"` on the spawner.

### 22.5 Mouse warping & focus stealing
- `input.warp_cursor_on_focus_change = "none" | "center" | "edge"`.
- `input.warp_cursor_on_workspace_change = bool`.
- **Focus stealing prevention**: new windows from background apps appear unfocused with `urgent` style until activated; configurable per-app rule (`steal_focus = true` opt-in).

### 22.6 Window urgency
Honor `xdg_activation_v1` activation tokens. Windows without a valid token that request focus get urgent state instead. Stays urgent until the user views them.

### 22.7 Color management & HDR
- Wide-gamut + HDR is gated behind `[monitor].hdr = true` + `bit_depth = 10`.
- `wp_color_management_v1` (when stable in upstream Wayland) for per-surface color spaces.
- Until then: nearest-neighbor compatibility with KDE/GNOME's experimental HDR pipelines.

### 22.8 Multi-GPU
- `griddy --gpu /dev/dri/card1` to pin primary render GPU.
- `[gpu]` block in config to map outputs to GPUs.
- DMABUF passthrough so client GPU ≠ compositor GPU works (NVIDIA + AMD mixed rigs).

### 22.9 Tablet / pen / touch input
- `tablet_v2` protocol for graphics tablets (Wacom, Huion).
- Per-tablet area mapping (`tablet.area = "stretch" | "letterbox" | "match-aspect"`).
- Touch protocol: tap, drag, long-press → context menu (via portal).
- Per-app touch translation: `touch_emulates_pointer = true` for apps that don't speak `wl_touch`.

### 22.10 Clipboard / primary selection
- `wlr-data-control-unstable-v1` for clipboard managers (cliphist, wl-clipboard).
- Configurable: clear primary on focus change, keep clipboard across X11/Wayland boundary.

### 22.11 Screen locking & session
- `ext-session-lock-v1` integration: any lock client (swaylock, hyprlock, gtklock) works.
- Configurable lock-on-suspend (`idle.lock_on_suspend = true`).
- DRM master handoff on VT switch.

### 22.12 Tearing
- `tearing_control_v1` honored per-window.
- Default: tearing only allowed for windows that explicitly opt in AND are in `TotalFullscreen` AND output `tearing = true`. Configurable.

### 22.13 Accessibility
- Larger borders / high contrast preset (`theme = "high-contrast"`).
- Screen reader bridge (Orca via `at-spi2`, requires running AT-SPI bus — not WM's job to ship but should not break it).
- Configurable animation reduction (`animations.respect_prefers_reduced_motion = true`).
- Keyboard-only operation: every overview interaction has a keybind equivalent.

### 22.14 Logging & debug
- `griddy -d` runs with `RUST_LOG=debug` and a debug overlay (frame times, damage rects, FBO usage) toggled by `$mod+Shift+d`.
- Log rotation in `$XDG_STATE_HOME/griddy/logs/`.
- `griddyctl debug` for runtime debug toggles (live damage visualization, FPS counter, surface tree dump).

### 22.15 Migration tooling
- `griddyctl import hyprland ~/.config/hypr/hyprland.conf` — translates keybinds, animations, decoration into GriddyWM TOML (best-effort; emits a report of unmapped settings).
- `griddyctl import niri ~/.config/niri/config.kdl` — same for Niri.
- `griddyctl import sway ~/.config/sway/config` — same for Sway/i3.

### 22.16 D-Bus integration
- Own `org.freedesktop.impl.portal.desktop.griddy` for the xdg-desktop-portal backend (screen capture, global shortcuts, settings, file chooser uses gtk/kde).
- `org.griddy.Compositor` for direct IPC over D-Bus (alternative to UNIX sockets, useful from sandboxed apps).
- Honor `org.freedesktop.login1` for suspend, lock, idle hints.

### 22.17 Power profiles
- Optional hook to `power-profiles-daemon`: switch to performance profile when a `content-type = game` window is `TotalFullscreen`, back to balanced on exit.

### 22.18 Internationalization
- IME via `input-method-v2` + `text-input-v3` (fcitx5, ibus).
- Localized OSD strings (compositor only; user-set workspace names obviously stay as-is).

### 22.19 Documentation & onboarding
- First-run experience: if no `config.toml` exists, copy a commented `/usr/share/griddy/default.toml` to `~/.config/griddy/` and show a one-time OSD with the default keybind cheatsheet.
- `griddyctl cheatsheet` opens an overlay with all currently-bound keys (sorted by category).
- Docs site at `griddywm.org` mirroring the layout of the Hyprland wiki: Configuration → Dispatchers → IPC → Animations → Shaders → Window Rules → Plugins → FAQ.

### 22.20 CI matrix
- Build & test on: latest stable Rust on Arch, Fedora rawhide, NixOS unstable, Debian testing, FreeBSD-CURRENT.
- Headless integration tests run on all of the above.
- Visual regression on one canonical Arch + Mesa configuration only (golden masters are GPU/driver-sensitive).
