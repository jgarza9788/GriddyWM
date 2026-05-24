# GriddyWM

A grid-based Wayland compositor written in Rust. Workspaces live on a 2D grid — navigate left/right/up/down (or diagonally) between them with a smooth slide animation. Windows occupy six geometric **tiled slots** and can be promoted to Fullscreen or TotalFullscreen without losing their place. An overview mode zooms out to show the whole grid at once.

![CI](https://github.com/griddywm/griddywm/actions/workflows/ci.yml/badge.svg)

> **Status: early development.** Core layout engine, IPC, config hot-reload, and the winit dev backend are working. The DRM/KMS backend, renderer polish, and overview UI are in progress.

---

## Features

- **Spatial grid workspaces** — NxN grid (3×3 default, up to 16×16). Navigate in any direction including diagonally. Wrap per-axis. Per-session history ring.
- **Six tiled slots** — `HalfLeft`, `HalfRight`, `QuarterTL`, `QuarterTR`, `QuarterBL`, `QuarterBR`. Compatible slots coexist; conflicting ones adapt or stack automatically.
- **Four window states** — `Tiled`, `Floating`, `Fullscreen`, `TotalFullscreen`. Promoting to fullscreen remembers the original slot and restores it on dismiss.
- **Stacking** — multiple windows per slot, cycled with `Super+Tab`. Stack-peek mode (`Super+\``) fans out the stack for visual inspection.
- **Overview mode** — zoomed-out view of the whole grid. Click to switch, drag to move windows between workspaces.
- **Minimap HUD** — persistent grid thumbnail for 4×4+ grids.
- **TOML config with hot-reload** — `Super+Shift+r` reloads without restarting. Bad configs never crash the compositor; error shown on-screen.
- **Powerful IPC** — two UNIX sockets (commands + events), JSON output, `griddyctl` CLI.
- **Shader pipeline** — per-event GLSL shaders (open/close/move/workspace-slide), per-window shaders, screen post-process shader.
- **Window rules** — match on `app_id`, `title`, `workspace`, XWayland status; set state, slot, opacity, pin, shader, and more.
- **Plugin system** — out-of-process `cdylib` plugins via a stable C ABI.
- **Session restore** — saves window assignments on shutdown, restores on next launch.
- **Shell-agnostic** — works with WayBar, Waybar, Noctalia, any `wlr-layer-shell` bar. Session file for LightDM / SDDM / GDM / greetd.

---

## Comparison

|                | Hyprland     | Niri         | Sway         | **GriddyWM**                        |
|----------------|--------------|--------------|--------------|--------------------------------------|
| Layout         | Dynamic tiling | Scrollable | Manual tiling | **Spatial 2D grid**                 |
| Workspaces     | Linear/named | Vertical     | Linear/named  | **NxN grid (col, row)**             |
| Backend        | Aquamarine   | Smithay      | wlroots       | **Smithay**                         |
| Language       | C++          | Rust         | C             | **Rust**                            |
| Overview       | Plugin       | Built-in     | No            | **Built-in core**                   |
| Config         | hyprlang/lua | KDL          | i3 syntax     | **TOML**                            |
| IPC            | 2 UNIX sockets | varlink    | i3 IPC        | **2 UNIX sockets (Hyprland-style)** |
| Shaders        | screen + plugins | open/close | none        | **Per-event + per-window + screen** |

---

## Default Keybinds

`$mod` is `Super` (Windows key). All binds are overridable in `config.toml`.

### Applications
| Bind | Action |
|------|--------|
| `Super+Return` | Launch kitty |
| `Super+d` | Launch fuzzel (launcher) |

### Window management
| Bind | Action |
|------|--------|
| `Super+q` | Close focused window |
| `Super+f` | Toggle fullscreen |
| `Super+Shift+f` | Toggle total-fullscreen |
| `Super+v` | Toggle floating |
| `Super+c` | Center floating window |
| `Super+Shift+Escape` | Exit total-fullscreen (global) |

### Focus
| Bind | Action |
|------|--------|
| `Super+h/j/k/l` | Focus left / down / up / right |

### Slot assignment
| Bind | Action |
|------|--------|
| `Super+Left` | Tile in left half |
| `Super+Right` | Tile in right half |
| `Super+w` | Enter **placement submap** |

**Placement submap** (`Super+w`, then):

| Key | Action |
|-----|--------|
| `h` | HalfLeft |
| `l` | HalfRight |
| `u` | QuarterTL |
| `i` | QuarterTR |
| `j` | QuarterBL |
| `k` | QuarterBR |
| `f` | Fullscreen |
| `F` | TotalFullscreen |
| `v` | Floating |
| `Escape` | Exit submap |

### Stacks
| Bind | Action |
|------|--------|
| `Super+Tab` | Next in stack |
| `Super+Shift+Tab` | Previous in stack |
| `Super+Alt+Tab` | Promote to top of stack |
| `Super+Alt+Shift+Tab` | Eject from stack (→ floating) |
| `Super+Alt+Up/Down` | Reorder in stack |
| `` Super+` `` (hold) | Stack-peek mode |

### Workspace navigation
| Bind | Action |
|------|--------|
| `Super+Ctrl+h/j/k/l` | Move workspace left / down / up / right |
| `Super+Ctrl+y/u/b/n` | Diagonal: NW / NE / SW / SE |
| `Super+Ctrl+o` | Workspace back (history) |
| `Super+Ctrl+i` | Workspace forward (history) |
| `Super+1–9` | Jump to workspace by index (row-major) |

### Move window to workspace
| Bind | Action |
|------|--------|
| `Super+Shift+Ctrl+h/j/k/l` | Move window left / down / up / right |
| `Super+Shift+1–9` | Move window to workspace by index |

### Overview & overlays
| Bind | Action |
|------|--------|
| `Super+o` | Toggle overview |
| `Super+m` | Toggle minimap |
| `Super+/` | Cheatsheet |
| `Super+,` | Show version OSD |
| `Super+Shift+d` | Toggle debug overlay |

### Compositor
| Bind | Action |
|------|--------|
| `Super+Shift+r` | Reload config |
| `Super+Shift+e` | Quit |

### Media keys (work during screen lock)
| Key | Action |
|-----|--------|
| `XF86AudioRaiseVolume` | Volume +5% |
| `XF86AudioLowerVolume` | Volume -5% |
| `XF86AudioMute` | Toggle mute |
| `XF86AudioMicMute` | Toggle mic mute |
| `XF86MonBrightnessUp/Down` | Brightness ±5% |
| `XF86AudioPlay/Next/Prev` | Media playback |
| `Print` | Screenshot (region) |

---

## Installation

### Dependencies (Arch)

```bash
sudo pacman -S rustup wayland wayland-protocols libxkbcommon mesa \
               libinput libseat systemd-libs pkgconf
```

### Dependencies (Fedora)

```bash
sudo dnf install rust cargo wayland-devel wayland-protocols-devel \
                 libxkbcommon-devel mesa-libEGL-devel libinput-devel \
                 libseat-devel systemd-devel pkgconf
```

### Dependencies (Ubuntu / Debian)

```bash
sudo apt-get install build-essential pkg-config libwayland-dev \
    libxkbcommon-dev libinput-dev libseat-dev libudev-dev libdrm-dev \
    libgbm-dev libegl-dev libgles2-mesa-dev libdbus-1-dev libsystemd-dev
```

### Build

```bash
git clone https://github.com/griddywm/griddywm
cd griddywm
cargo build --release
sudo install -Dm755 target/release/griddy    /usr/bin/griddy
sudo install -Dm755 target/release/griddyctl /usr/bin/griddyctl
sudo install -Dm644 dist/griddy.desktop      /usr/share/wayland-sessions/griddy.desktop
```

---

## Configuration

Config is read from the first of:

1. `$GRIDDY_CONFIG`
2. `$XDG_CONFIG_HOME/griddy/config.toml`
3. `~/.config/griddy/config.toml`
4. `/etc/griddy/config.toml`

On first run with no config, the default config is copied to `~/.config/griddy/config.toml` automatically.

A fully annotated default config lives at [`dist/default.toml`](dist/default.toml). Key sections:

```toml
[grid]
cols = 3
rows = 3
wrap_x = true
wrap_y = true
workspace_sync = "unsynced"   # "synced" | "unsynced"

[input]
follow_mouse = "loose"        # "off" | "loose" | "strict"
natural_scroll = true
tap_to_click = true

[windows]
on_slot_conflict = "stack"    # "stack" | "swap" | "kick-to-floating"

[animations]
open_duration_ms = 160
close_duration_ms = 120

[[bind]]
key = "t"
mods = ["$mod"]
action = "exec alacritty"
```

Split into multiple files via `imports`:

```toml
# config.toml
imports = ["keybinds.toml", "rules.toml", "theme.toml"]
```

Hot-reload with `Super+Shift+r` or `griddyctl reload`. Parse errors show a red OSD overlay; the previous valid config stays active.

### Window rules

```toml
[[rule]]
match.app_id = "mpv"
action.state = "fullscreen"

[[rule]]
match.title = "Picture-in-Picture"
action.floating_geom = { x = 20, y = 20, w = 480, h = 270 }
action.pin = true

[[rule]]
match.app_id = "^steam_app_"
action.state = "total-fullscreen"
action.above_total_fullscreen = false
```

---

## IPC — griddyctl

```bash
# Query state
griddyctl get windows
griddyctl get activewindow
griddyctl get workspaces
griddyctl get grid
griddyctl monitors

# Dispatch actions
griddyctl dispatch workspace-left
griddyctl dispatch workspace 1,2
griddyctl dispatch slot-half-left
griddyctl dispatch exec firefox

# Runtime config changes (no reload needed)
griddyctl keyword animations.open_duration_ms 80
griddyctl keyword grid.wrap_x true
griddyctl keyword input.follow_mouse strict

# Query a config value
griddyctl getoption grid.cols
griddyctl getoption grid.workspace_sync

# Set per-window properties
griddyctl setprop 7 opacity 0.9
griddyctl setprop 7 is_urgent true

# Shaders
griddyctl shader set --window 7 ~/shaders/crt.glsl
griddyctl shader screen ~/shaders/blue-light.glsl
griddyctl shader clear

# Resize the grid live
griddyctl grid resize 4 4

# Batch multiple commands
griddyctl --batch "dispatch slot-half-left ; dispatch focus-right"

# JSON output
griddyctl -j get windows

# Subscribe to events
griddyctl subscribe
griddyctl subscribe workspace_changed window_focus

# Reload config
griddyctl reload
griddyctl reload theme

# Import config from another compositor (best-effort)
griddyctl import hyprland ~/.config/hypr/hyprland.conf
griddyctl import sway ~/.config/sway/config
griddyctl import niri ~/.config/niri/config.kdl
```

### Events

The event socket emits one line per event in `name>>data\n` format:

| Event | Data |
|-------|------|
| `workspace_changed` | `col,row,monitor` |
| `window_opened` | `id,app_id,col,row,state,slot` |
| `window_closed` | `id` |
| `window_focus` | `id` |
| `window_placed` | `id,policy,slot` |
| `window_state_changed` | `id,state` |
| `submap_changed` | `name` |
| `config_reloaded` | `which` |
| `config_error` | `path:line:col message` |
| `shader_loaded` | `category,path` |
| `workspace_sync_changed` | `synced\|unsynced` |
| `safe_mode_entered` | `reason` |

Full catalogue in [`griddy/src/ipc/events.rs`](griddy/src/ipc/events.rs).

---

## Compatibility

- **Display managers:** LightDM, SDDM, GDM, greetd, ly — standard `.desktop` session file.
- **Bars / shells:** WayBar, Noctalia, Quickshell — via `wlr-layer-shell` + `ext-workspace-v1`.
- **Launchers:** Fuzzel, Wofi, Rofi-Wayland, Anyrun, Walker.
- **Wallpaper:** swaybg, swww, mpvpaper, hyprpaper.
- **Idle management:** swayidle, hypridle (standard idle-notify protocol).
- **Notifications:** Mako, Dunst, SwayNC, Fnott.
- **Clipboard:** wl-clipboard, cliphist.
- **Screen lock:** swaylock, hyprlock, gtklock (`ext-session-lock-v1`).

---

## License

MIT — see [LICENSE](LICENSE).
