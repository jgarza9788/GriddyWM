//! Keybind lookup table (§9). Parses [[bind]] config entries into a fast
//! (modifier-mask, keysym) → Action table. Also builds per-submap tables.

pub mod dispatcher;

use std::collections::HashMap;

use smithay::input::keyboard::ModifiersState;
use xkbcommon::xkb;

use crate::config::types::{BindConfig, SubBindConfig, SubmapConfig};
pub use dispatcher::Action;

// ─── Bind table ───────────────────────────────────────────────────────────────

pub struct BindTable {
    entries: Vec<BindEntry>,
    /// Mouse-button binds (§9.3): `Mouse:Left`, `Mouse:Right`, etc.
    mouse_entries: Vec<MouseBindEntry>,
}

struct BindEntry {
    /// Modifier bitmask: bit0=Logo/Super  bit1=Alt  bit2=Ctrl  bit3=Shift
    mods_mask: u32,
    /// Raw xkb keysym value.
    sym: u32,
    /// Original key name string (for cheatsheet display).
    key_name: String,
    action: Action,
    /// Original action + args string (for cheatsheet display).
    action_str: String,
    passthrough: bool,
    /// Fires even when screen is locked (§9.1 flag).
    locked: bool,
    /// Fires even when a TotalFullscreen window has input grab (§9.1 flag).
    global: bool,
    /// If true, action fires repeatedly while the key is held (§9).
    repeat: bool,
}

struct MouseBindEntry {
    mods_mask: u32,
    /// Linux input button code (BTN_LEFT=0x110, BTN_RIGHT=0x111, etc.).
    button_code: u32,
    action: Action,
}

/// Map `Mouse:*` / `Scroll:*` key names (§9.3) to Linux input button codes.
fn mouse_key_to_button(key: &str) -> Option<u32> {
    match key {
        "Mouse:Left"    => Some(0x110), // BTN_LEFT
        "Mouse:Right"   => Some(0x111), // BTN_RIGHT
        "Mouse:Middle"  => Some(0x112), // BTN_MIDDLE
        "Mouse:Forward" => Some(0x115), // BTN_FORWARD
        "Mouse:Back"    => Some(0x116), // BTN_BACK
        "Scroll:Up"     => Some(0x150), // synthetic: scroll-up
        "Scroll:Down"   => Some(0x151), // synthetic: scroll-down
        "Scroll:Left"   => Some(0x152), // synthetic: scroll-left
        "Scroll:Right"  => Some(0x153), // synthetic: scroll-right
        _ => None,
    }
}

impl BindTable {
    /// Build a table from config [[bind]] entries.
    pub fn from_config(binds: &[BindConfig]) -> Self {
        let mut entries = Vec::new();
        let mut mouse_entries = Vec::new();
        for bind in binds {
            if bind.key.is_empty() || bind.action.is_empty() {
                continue;
            }
            // Mouse/scroll binds use synthetic key names (§9.3).
            if let Some(button_code) = mouse_key_to_button(&bind.key) {
                let Some(action) = dispatcher::parse_action(&bind.action, &bind.args) else {
                    tracing::warn!(action = %bind.action, "Unknown action — skipping mouse bind");
                    continue;
                };
                mouse_entries.push(MouseBindEntry {
                    mods_mask: modset_from_vec(&bind.mods),
                    button_code,
                    action,
                });
                continue;
            }
            let sym_val = xkb::keysym_from_name(&bind.key, xkb::KEYSYM_NO_FLAGS);
            if sym_val.raw() == 0 {
                tracing::warn!(key = %bind.key, "Unrecognized XKB key name — skipping bind");
                continue;
            }
            let Some(action) = dispatcher::parse_action(&bind.action, &bind.args) else {
                tracing::warn!(action = %bind.action, "Unknown action — skipping bind");
                continue;
            };
            let action_str = if bind.args.is_empty() {
                bind.action.clone()
            } else {
                format!("{} {}", bind.action, bind.args.join(" "))
            };
            entries.push(BindEntry {
                mods_mask: modset_from_vec(&bind.mods),
                sym: sym_val.raw(),
                key_name: bind.key.clone(),
                action,
                action_str,
                passthrough: bind.passthrough,
                locked: bind.locked,
                global: bind.global,
                repeat: bind.repeat,
            });
        }
        Self { entries, mouse_entries }
    }

    /// Build from submap [[bind]] entries (subset of BindConfig fields).
    fn from_sub_config(binds: &[SubBindConfig]) -> Self {
        let converted: Vec<BindConfig> = binds
            .iter()
            .map(|b| BindConfig {
                mods: b.mods.clone(),
                key: b.key.clone(),
                action: b.action.clone(),
                args: b.args.clone(),
                repeat: b.repeat,
                ..Default::default()
            })
            .collect();
        Self::from_config(&converted)
    }

    /// Build a table using the spec §9.1 defaults.
    pub fn from_defaults(mod_key: &str) -> Self {
        Self::from_config(&default_binds(mod_key))
    }

    /// Build the default `[[bind_release]]` table (§9 hold-to-peek binds).
    pub fn default_releases(mod_key: &str) -> Self {
        Self::from_config(&default_release_binds(mod_key))
    }

    /// Return all binds in human-readable form for the cheatsheet IPC command (§22.19).
    /// Each element: (mods_str, key_name, action_str, locked, global).
    pub fn for_cheatsheet(&self) -> Vec<(String, String, String, bool, bool)> {
        let mut out: Vec<(String, String, String, bool, bool)> = self.entries.iter().map(|e| {
            let mods = mask_to_mods_str(e.mods_mask);
            (mods, e.key_name.clone(), e.action_str.clone(), e.locked, e.global)
        }).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        out
    }

    /// Look up a keysym+modifiers and return `(action, locked, global, passthrough, repeat)`, if any.
    pub fn lookup_flags(&self, sym: u32, mods: &ModifiersState) -> Option<(Action, bool, bool, bool, bool)> {
        let mask = mods_to_mask(mods);
        self.entries
            .iter()
            .find(|e| e.sym == sym && e.mods_mask == mask)
            .map(|e| (e.action.clone(), e.locked, e.global, e.passthrough, e.repeat))
    }

    /// Convenience: look up and return the Action only (ignores all flags).
    pub fn lookup(&self, sym: u32, mods: &ModifiersState) -> Option<Action> {
        self.lookup_flags(sym, mods).map(|(a, _, _, _, _)| a)
    }

    /// Look up a mouse button code + modifiers (§9.3).  Returns the action if any bind matches.
    pub fn lookup_button(&self, button_code: u32, mods: &ModifiersState) -> Option<Action> {
        let mask = mods_to_mask(mods);
        self.mouse_entries
            .iter()
            .find(|e| e.button_code == button_code && e.mods_mask == mask)
            .map(|e| e.action.clone())
    }
}

// ─── Submap table ─────────────────────────────────────────────────────────────

/// A named submap's key bindings + exit behaviour (§8.7).
pub struct SubBindTable {
    pub table: BindTable,
    /// Precomputed xkb keysyms that always exit the submap.
    exit_syms: Vec<u32>,
    /// Exit the submap when any unrecognized key is pressed.
    pub exit_on_unhandled: bool,
    /// Exit the submap after any recognized action fires.
    pub exit_after_action: bool,
}

impl SubBindTable {
    pub fn is_exit_key(&self, sym: u32) -> bool {
        self.exit_syms.contains(&sym)
    }

    pub fn lookup(&self, sym: u32, mods: &ModifiersState) -> Option<Action> {
        self.table.lookup(sym, mods)
    }
}

/// Build all submap tables from config, merging with the built-in submaps.
pub fn build_submap_tables(submaps: &[SubmapConfig], mod_key: &str) -> HashMap<String, SubBindTable> {
    let _ = mod_key; // reserved for future use (built-in submaps don't depend on it)
    let mut map: HashMap<String, SubBindTable> = HashMap::new();

    // Insert built-in submaps first.
    for (name, table) in builtin_submaps() {
        map.insert(name, table);
    }

    // User-defined submaps override built-ins by name.
    for cfg in submaps {
        let exit_syms = cfg
            .exit_keys
            .iter()
            .map(|k| xkb::keysym_from_name(k, xkb::KEYSYM_NO_FLAGS).raw())
            .filter(|&s| s != 0)
            .collect();
        map.insert(
            cfg.name.clone(),
            SubBindTable {
                table: BindTable::from_sub_config(&cfg.binds),
                exit_syms,
                exit_on_unhandled: cfg.exit_on_unhandled,
                exit_after_action: cfg.exit_after_action,
            },
        );
    }

    map
}

fn builtin_submaps() -> Vec<(String, SubBindTable)> {
    vec![
        ("placement".into(), placement_submap()),
        ("resize".into(),    resize_submap()),
    ]
}

fn placement_submap() -> SubBindTable {
    let binds = vec![
        sb(&[], "f",      "state-fullscreen-toggle"),
        sb(&["Shift"], "f", "state-total-fullscreen-toggle"),
        sb(&[], "h",      "slot-half-left"),
        sb(&[], "l",      "slot-half-right"),
        sb(&[], "u",      "slot-quarter-tl"),
        sb(&[], "i",      "slot-quarter-tr"),
        sb(&[], "j",      "slot-quarter-bl"),
        sb(&[], "k",      "slot-quarter-br"),
        sb(&[], "v",      "state-floating-toggle"),
    ];
    SubBindTable {
        table: BindTable::from_config(&binds),
        exit_syms: vec![xkb::keysym_from_name("Escape", xkb::KEYSYM_NO_FLAGS).raw()],
        exit_on_unhandled: true,
        exit_after_action: true,
    }
}

fn resize_submap() -> SubBindTable {
    let mut binds: Vec<BindConfig> = vec![
        sb(&[], "h", "resize-active"),
        sb(&[], "l", "resize-active"),
        sb(&[], "j", "resize-active"),
        sb(&[], "k", "resize-active"),
    ];
    // Mark resize binds as repeating.
    for b in &mut binds {
        b.repeat = true;
    }
    let escape_sym = xkb::keysym_from_name("Escape", xkb::KEYSYM_NO_FLAGS).raw();
    let return_sym = xkb::keysym_from_name("Return", xkb::KEYSYM_NO_FLAGS).raw();
    SubBindTable {
        table: BindTable::from_config(&[
            sub_bind("h", "resize-active", &["-20", "0"]),
            sub_bind("l", "resize-active", &["20",  "0"]),
            sub_bind("k", "resize-active", &["0", "-20"]),
            sub_bind("j", "resize-active", &["0",  "20"]),
        ]),
        exit_syms: vec![escape_sym, return_sym],
        exit_on_unhandled: false,
        exit_after_action: false,
    }
}

fn sb(mods: &[&str], key: &str, action: &str) -> BindConfig {
    BindConfig {
        mods: mods.iter().map(|s| s.to_string()).collect(),
        key: key.to_string(),
        action: action.to_string(),
        ..Default::default()
    }
}

fn sub_bind(key: &str, action: &str, args: &[&str]) -> BindConfig {
    BindConfig {
        key: key.to_string(),
        action: action.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        repeat: true,
        ..Default::default()
    }
}

// ─── Gesture table (§9.4) ────────────────────────────────────────────────────

struct GestureEntry {
    fingers: u8,
    direction: String,
    action: Action,
}

/// Lookup table for `[[gesture]]` binds (§9.4).
pub struct GestureTable {
    entries: Vec<GestureEntry>,
}

impl GestureTable {
    pub fn from_config(gestures: &[crate::config::types::GestureConfig]) -> Self {
        let mut entries = Vec::new();
        for g in gestures {
            if g.action.is_empty() || g.direction.is_empty() { continue; }
            let Some(action) = dispatcher::parse_action(&g.action, &g.args) else {
                tracing::warn!(action = %g.action, "Unknown gesture action — skipping");
                continue;
            };
            entries.push(GestureEntry {
                fingers: g.fingers,
                direction: g.direction.clone(),
                action,
            });
        }
        Self { entries }
    }

    /// Built-in defaults: 4-finger up → overview-toggle; 3-finger left/right → workspace nav.
    pub fn from_defaults() -> Self {
        use crate::config::types::GestureConfig;
        Self::from_config(&[
            GestureConfig { fingers: 4, direction: "up".into(),    action: "overview-toggle".into(), args: vec![] },
            GestureConfig { fingers: 4, direction: "down".into(),  action: "overview-toggle".into(), args: vec![] },
            GestureConfig { fingers: 3, direction: "left".into(),  action: "workspace-right".into(), args: vec![] },
            GestureConfig { fingers: 3, direction: "right".into(), action: "workspace-left".into(),  args: vec![] },
        ])
    }

    /// Look up a gesture by finger count and direction.
    pub fn lookup(&self, fingers: u8, direction: &str) -> Option<Action> {
        self.entries
            .iter()
            .find(|e| e.fingers == fingers && e.direction == direction)
            .map(|e| e.action.clone())
    }
}

// ─── Modifier helpers ─────────────────────────────────────────────────────────

fn mods_to_mask(mods: &ModifiersState) -> u32 {
    let mut m = 0u32;
    if mods.logo  { m |= 1 << 0; }
    if mods.alt   { m |= 1 << 1; }
    if mods.ctrl  { m |= 1 << 2; }
    if mods.shift { m |= 1 << 3; }
    m
}

fn mask_to_mods_str(mask: u32) -> String {
    let mut parts = Vec::new();
    if mask & (1 << 0) != 0 { parts.push("Super"); }
    if mask & (1 << 1) != 0 { parts.push("Alt"); }
    if mask & (1 << 2) != 0 { parts.push("Ctrl"); }
    if mask & (1 << 3) != 0 { parts.push("Shift"); }
    parts.join("+")
}

fn modset_from_vec(mods: &[String]) -> u32 {
    let mut m = 0u32;
    for s in mods {
        match s.as_str() {
            "Super" | "Logo" | "Win" | "Mod4" => m |= 1 << 0,
            "Alt"   | "Mod1"                  => m |= 1 << 1,
            "Ctrl"  | "Control"               => m |= 1 << 2,
            "Shift"                           => m |= 1 << 3,
            other => tracing::warn!(modifier = %other, "Unknown modifier name"),
        }
    }
    m
}

// ─── Default keybinds (§9.1) ─────────────────────────────────────────────────

fn b(mods: &[&str], key: &str, action: &str, args: &[&str]) -> BindConfig {
    BindConfig {
        mods: mods.iter().map(|s| s.to_string()).collect(),
        key: key.to_string(),
        action: action.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

/// Locked bind — fires even during screen lock (for media / brightness / screenshot keys).
fn bl(key: &str, action: &str, args: &[&str]) -> BindConfig {
    BindConfig {
        key: key.to_string(),
        action: action.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        locked: true,
        ..Default::default()
    }
}

/// Global bind — fires even when a TotalFullscreen window has input grab.
fn bg(mods: &[&str], key: &str, action: &str, args: &[&str]) -> BindConfig {
    BindConfig {
        mods: mods.iter().map(|s| s.to_string()).collect(),
        key: key.to_string(),
        action: action.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        global: true,
        ..Default::default()
    }
}

fn default_binds(mod_key: &str) -> Vec<BindConfig> {
    let m = mod_key;
    let mut v = vec![
        // ── Applications ──────────────────────────────────────────────────────
        b(&[m],          "Return", "exec",                      &["kitty"]),
        b(&[m],          "d",      "exec",                      &["fuzzel"]),
        // ── Window management ─────────────────────────────────────────────────
        b(&[m],          "q",      "close-window",              &[]),
        b(&[m, "Shift"], "e",      "quit",                      &[]),
        b(&[m, "Shift"], "r",      "reload-config",             &[]),
        // ── Focus navigation ──────────────────────────────────────────────────
        b(&[m],          "h",      "focus-left",                &[]),
        b(&[m],          "j",      "focus-down",                &[]),
        b(&[m],          "k",      "focus-up",                  &[]),
        b(&[m],          "l",      "focus-right",               &[]),
        // ── Stack cycling ─────────────────────────────────────────────────────
        b(&[m],          "Tab",    "stack-next",                &[]),
        b(&[m, "Shift"], "Tab",    "stack-prev",                &[]),
        b(&[m, "Alt"],   "Tab",    "stack-promote",             &[]),
        b(&[m, "Alt", "Shift"], "Tab", "stack-collapse",        &[]),
        b(&[m, "Alt"],   "Up",     "stack-move-up",             &[]),
        b(&[m, "Alt"],   "Down",   "stack-move-down",           &[]),
        // ── Window states ─────────────────────────────────────────────────────
        b(&[m],          "f",      "state-fullscreen-toggle",         &[]),
        b(&[m, "Shift"], "f",      "state-total-fullscreen-toggle",   &[]),
        b(&[m, "Shift"], "Escape", "total-fullscreen-exit",           &[]),
        b(&[m],          "v",      "state-floating-toggle",           &[]),
        // ── Overlays ──────────────────────────────────────────────────────────
        b(&[m],          "m",      "minimap-toggle",                  &[]),
        b(&[m, "Shift"], "d",      "debug-overlay-toggle",            &[]),
        b(&[m],          "slash",  "cheatsheet-toggle",               &[]),
        b(&[m],          "comma",  "osd-show",                        &["griddy v0.1"]),
        // ── Slot assignment ───────────────────────────────────────────────────
        b(&[m],          "Left",   "slot-half-left",            &[]),
        b(&[m],          "Right",  "slot-half-right",           &[]),
        b(&[m],          "c",      "center-floating",           &[]),
        // ── Submaps ───────────────────────────────────────────────────────────
        b(&[m],          "w",      "submap",                    &["placement"]),
        b(&[m],          "r",      "submap",                    &["resize"]),
        // ── Workspace navigation ──────────────────────────────────────────────
        b(&[m, "Ctrl"],  "h",      "workspace-left",            &[]),
        b(&[m, "Ctrl"],  "j",      "workspace-down",            &[]),
        b(&[m, "Ctrl"],  "k",      "workspace-up",              &[]),
        b(&[m, "Ctrl"],  "l",      "workspace-right",           &[]),
        b(&[m, "Ctrl"],  "y",      "workspace-nw",              &[]),
        b(&[m, "Ctrl"],  "u",      "workspace-ne",              &[]),
        b(&[m, "Ctrl"],  "b",      "workspace-sw",              &[]),
        b(&[m, "Ctrl"],  "n",      "workspace-se",              &[]),
        b(&[m, "Ctrl"],  "o",      "workspace-back",            &[]),
        b(&[m, "Ctrl"],  "i",      "workspace-forward",         &[]),
        // ── Move window to workspace ──────────────────────────────────────────
        b(&[m, "Shift", "Ctrl"], "h", "move-window-direction", &["left"]),
        b(&[m, "Shift", "Ctrl"], "j", "move-window-direction", &["down"]),
        b(&[m, "Shift", "Ctrl"], "k", "move-window-direction", &["up"]),
        b(&[m, "Shift", "Ctrl"], "l", "move-window-direction", &["right"]),
        // ── Overview (§7.2) ───────────────────────────────────────────────────
        b(&[m], "o", "overview-toggle", &[]),
        // ── Stack-peek (§6.8 — press enters peek, release bind exits it) ──────
        b(&[m], "grave", "stack-peek", &[]),
    ];

    // workspace-index 1..9 and move-window-to-index 1..9
    for n in 1u8..=9 {
        let ns = n.to_string();
        v.push(b(&[m],          &ns, "workspace-index",       &[&ns]));
        v.push(b(&[m, "Shift"], &ns, "move-window-to-index",  &[&ns]));
    }

    // ── Hardware / media keys (locked = true — fire during screen lock) ────────
    v.push(bl("XF86AudioRaiseVolume",  "exec", &["wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%+"]));
    v.push(bl("XF86AudioLowerVolume",  "exec", &["wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-"]));
    v.push(bl("XF86AudioMute",         "exec", &["wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"]));
    v.push(bl("XF86AudioMicMute",      "exec", &["wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle"]));
    v.push(bl("XF86MonBrightnessUp",   "exec", &["brightnessctl set 5%+"]));
    v.push(bl("XF86MonBrightnessDown", "exec", &["brightnessctl set 5%-"]));
    v.push(bl("XF86AudioPlay",         "exec", &["playerctl play-pause"]));
    v.push(bl("XF86AudioNext",         "exec", &["playerctl next"]));
    v.push(bl("XF86AudioPrev",         "exec", &["playerctl previous"]));
    v.push(bl("Print",                 "screenshot", &["region"]));

    // ── Global binds (fire even during TotalFullscreen input grab) ─────────────
    v.push(bg(&[m, "Shift"], "Escape", "total-fullscreen-exit", &[]));
    v.push(bg(&[m, "Shift"], "e",      "quit",                  &[]));

    v
}

/// Default `[[bind_release]]` entries: hold-to-peek binds (§9).
fn default_release_binds(mod_key: &str) -> Vec<BindConfig> {
    let m = mod_key;
    vec![
        // `$mod+grave` held → stack-peek fan-out; released → collapse
        b(&[m], "grave", "stack-peek", &[]),
        // `$mod+grave` pressed also enters peek (we add matching press bind
        // to the main table separately). Here only the release exit is needed.
    ]
    // Note: overview-peek ($mod alone) cannot be expressed as a keysym in a
    // standard bind table; the held-Super detection is handled in the input
    // backend directly using modifier-release tracking.
}
