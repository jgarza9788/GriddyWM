# Contributing to GriddyWM

## Project layout

```
griddy/          compositor binary (the Wayland compositor itself)
  src/
    backend/     winit (dev) and DRM/KMS backend wrappers
    config/      TOML config types, hot-reload, monitors, rules, theme
    grid/        layout engine — slots, states, stacks, window registry
    handlers/    Smithay protocol handlers (compositor, xdg_shell, layer_shell, …)
    ipc/         UNIX socket server, command dispatch, event types
    keybind/     keybind table, dispatcher (Action enum + handlers)
    plugins/     plugin ABI loader (feature-gated behind `plugin-abi`)
    animate.rs   animation state machine
    session.rs   session save / restore
    state.rs     GlobalState — the single root of all compositor state

griddyctl/       control CLI (talks to the compositor over IPC sockets)
  src/main.rs    clap CLI, socket plumbing, config import translators

dist/            install artifacts
  default.toml   annotated default config (copy to ~/.config/griddy/config.toml)
  griddy.1       griddy(1) man page
  griddyctl.1    griddyctl(1) man page
  themes/        built-in theme presets

GriddyWM-spec.md full project specification — the authoritative design document
PROGRESS.md      session-by-session build log
```

## Building

### Requirements

**Rust:** stable toolchain (1.85+, edition 2024). Install via [rustup](https://rustup.rs).

**System libraries:**

```bash
# Arch
sudo pacman -S wayland wayland-protocols libxkbcommon libinput libseat \
               mesa systemd-libs pkgconf

# Fedora
sudo dnf install wayland-devel wayland-protocols-devel libxkbcommon-devel \
                 libinput-devel libseat-devel mesa-libEGL-devel systemd-devel pkgconf

# Ubuntu / Debian
sudo apt-get install pkg-config libwayland-dev libxkbcommon-dev libinput-dev \
    libseat-dev libudev-dev libdrm-dev libgbm-dev libegl-dev \
    libgles2-mesa-dev libdbus-1-dev libsystemd-dev
```

### Compile

```bash
cargo build                          # debug build (faster compile)
cargo build --release                # release build
```

### Feature flags

| Flag | Default | Description |
|------|---------|-------------|
| `xwayland` | on | XWayland support |
| `plugin-abi` | on | Dynamic plugin loading (`cdylib`) |
| `vulkan` | off | Vulkan renderer (future) |
| `systemd` | on | logind / systemd-dbus integration |
| `elogind` | off | elogind alternative to systemd |

```bash
cargo build --no-default-features --features "xwayland"
```

### Running (dev mode — inside an existing Wayland session)

```bash
cargo run -- --config dist/default.toml
```

This uses the winit backend and opens a window within your current session. IPC sockets are created at `$XDG_RUNTIME_DIR/griddy/$GRIDDY_INSTANCE_SIGNATURE/`.

```bash
# In another terminal, test IPC
export GRIDDY_INSTANCE_SIGNATURE=$(ls $XDG_RUNTIME_DIR/griddy/)
griddyctl get windows
griddyctl dispatch workspace-right
```

## Tests

```bash
cargo test --workspace           # run all tests
cargo test -p griddy             # compositor tests only
cargo test grid::                # filter by module prefix
```

Tests live next to the code they cover in `#[cfg(test)]` modules. The test suite is pure unit tests — no display server or GPU required.

### What's tested

- `grid::` — layout solver, slot math, wrap/boundary navigation, grid resize
- `config::rules::` — window rule matching (exact, glob, regex)
- `config::monitors::` — monitor config parsing
- `config::types::` — config deserialization, defaults
- `ipc::events::` — event wire format (`name>>data\n`)
- `ipc::commands::` — command parsing helpers
- `session::` — session save/restore round-trip

### CI

GitHub Actions runs on every push/PR to `main`/`master`:

- **Build & Test** — `cargo build --locked` + `cargo test --workspace --locked` (blocks merge)
- **Clippy** — `cargo clippy -- -D warnings` (blocks merge)
- **Format** — `cargo fmt --check` (informational, does not block)

See [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

## Code style

- **No `unsafe`** outside of smithay/libloading FFI boundaries.
- **No `unwrap()`** in non-test code. Propagate with `?` or log-and-continue.
- **No comments** that describe *what* the code does — only *why* (non-obvious constraints, workarounds, invariants).
- Run `cargo clippy` before submitting; the CI treats warnings as errors.
- Formatting: `cargo fmt --all` (standard rustfmt defaults, no custom config).

## Architecture notes

### GlobalState

Everything lives in `GlobalState` (`griddy/src/state.rs`). It's the single root struct — Smithay requires this because it's passed by `&mut self` through every protocol handler. Adding new state fields means adding them here.

### Action dispatch pipeline

```
keybind press
  → KeybindTable::lookup()
  → dispatcher::parse_action()
  → dispatcher::dispatch(action, &mut state)
  → state mutations + state.pending_events.push(Event::…)
```

IPC commands follow the same path via `ipc::commands::handle()` → `dispatcher::dispatch()`.

### Events

Events are queued in `state.pending_events: Vec<Event>` during a frame and flushed to subscribers at the end of the event loop tick. Never write directly to the socket from inside a handler.

### Config hot-reload

`state.reload_config_if_changed(force)` re-parses all config files, diffs against the current config, and calls `grid.update_from_config()` + `keybind_table.rebuild()`. A failed parse leaves the current config intact and pushes a `ConfigError` event.

## Adding a new IPC command

1. Add a match arm in `ipc::commands::handle()` with the command name.
2. Write the handler function `fn my_cmd(args: &str, json: bool, state: &mut GlobalState) -> String`.
3. If it's a new `griddyctl` subcommand, add it to the `Command` enum in `griddyctl/src/main.rs` and wire it up in `main()`.
4. Document it in `dist/griddyctl.1`.

## Adding a new dispatcher action

1. Add a variant to `Action` in `keybind/dispatcher.rs`.
2. Add a match arm in `parse_action()`.
3. Add a match arm in `dispatch()` that mutates state and/or pushes events.
4. Add the action name to the default binds in `keybind/mod.rs::default_binds()` if appropriate.
5. Update `dist/griddyctl.1` under `dispatch`.

## Reporting issues

Open an issue at <https://github.com/griddywm/griddywm/issues>. Include:

- `griddy --version` output
- Compositor logs (`RUST_LOG=debug griddy 2>griddy.log`)
- `griddyctl -j get windows` and `griddyctl -j get activewindow` if window-management related
- Steps to reproduce
