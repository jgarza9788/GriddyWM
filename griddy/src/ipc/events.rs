//! IPC event types and wire format (§12.2).
//!
//! Wire format: `<eventname>>><data>\n`

#[derive(Debug, Clone)]
pub enum Event {
    WorkspaceChanged { col: u8, row: u8 },
    WorkspaceCreated { col: u8, row: u8, name: String },
    WorkspaceRenamed { col: u8, row: u8, old: String, new: String },
    /// Fired when workspace_sync mode changes (synced | unsynced).
    WorkspaceSyncChanged { synced: bool },
    WindowOpened { id: u64, app_id: String, workspace_col: u8, workspace_row: u8, state: String, slot: String },
    WindowClosed { id: u64 },
    WindowMoved { id: u64, col: u8, row: u8 },
    /// Emitted right after the placement policy assigns a window (§6.9).
    WindowPlaced { id: u64, policy_matched: String, slot_assigned: String },
    WindowFocus { id: u64 },
    WindowTitle { id: u64, title: String },
    WindowAppId { id: u64, app_id: String },
    WindowStateChanged { id: u64, state: String },
    WindowSlotChanged { id: u64, slot: String },
    /// Emitted when a Half slot adapts to a complementary Quarter (§6.5.1).
    WindowSlotAdapted { id: u64, requested: String, actual: String },
    /// Emitted when a Fullscreen-state window adapts to a tiled slot (§6.5.1).
    WindowStateAdapted { id: u64, requested_state: String, actual_state: String, actual_slot: String },
    /// Emitted when move-window-direction skips a TotalFullscreen workspace (§6.7.1).
    WindowMoveSkipped { id: u64, requested_col: u8, requested_row: u8, actual_col: u8, actual_row: u8 },
    /// Emitted when move-window-to is refused due to a TotalFullscreen workspace (§6.7.1).
    WindowMoveRefused { id: u64, requested_col: u8, requested_row: u8, reason: String },
    /// Emitted when the top window or size of a slot stack changes (§6.8).
    WindowStackChanged { slot: String, top_id: u64, size: usize },
    /// Emitted when stack reorder changes the order (stack-move-up/down/flip) (§6.8).
    WindowStackReordered { slot: String, order_csv: String },
    SubMapChanged { name: String },
    ConfigReloaded { which: String },
    ConfigError { message: String },
    ThemeReloaded,
    /// Emitted when a per-event or per-window shader is successfully compiled.
    ShaderLoaded { category: String, path: String },
    /// Emitted when shader compilation fails (§11.2).
    ShaderError { category: String, path: String, log: String },
    OverviewOpened,
    OverviewClosed,
    ViewModeChanged { mode: String },
    MinimapToggled { visible: bool },
    MonitorAdded { name: String, width: u32, height: u32, scale: f64 },
    MonitorRemoved { name: String },
    MonitorConfigChanged { name: String },
    KeyboardLayoutChanged { device: String, layout: String },
    IdleTimeout { after_seconds: u64 },
    IdleResume { after_seconds: u64 },
    Urgent { id: u64 },
    SessionLocked,
    SessionUnlocked,
    /// Fired when crash-recovery enters safe mode (§22.2).
    SafeModeEntered { reason: String },
    /// Fired if no org.freedesktop.Notifications daemon is reachable (§14.2).
    NotificationDaemonMissing,
    PluginError { name: String, message: String },
    PluginLoaded { name: String },
    /// Emitted when a tiled window is forced to Floating because the slot is
    /// smaller than the window's declared min_size (§6.13).
    WindowSizeConstraintFloat { id: u64, slot: String, min_w: i32, min_h: i32, slot_w: i32, slot_h: i32 },
}

impl Event {
    pub fn format(&self) -> String {
        let (name, data): (&str, String) = match self {
            Event::WorkspaceChanged { col, row } =>
                ("workspace_changed", format!("{col},{row},winit")),
            Event::WorkspaceCreated { col, row, name } =>
                ("workspace_created", format!("{col},{row},winit,{name}")),
            Event::WorkspaceRenamed { col, row, old, new } =>
                ("workspace_renamed", format!("{col},{row},winit,{old},{new}")),
            Event::WorkspaceSyncChanged { synced } =>
                ("workspace_sync_changed", if *synced { "synced" } else { "unsynced" }.into()),
            Event::WindowOpened { id, app_id, workspace_col, workspace_row, state, slot } =>
                ("window_opened", format!("{id},{app_id},{workspace_col},{workspace_row},{state},{slot}")),
            Event::WindowClosed { id } =>
                ("window_closed", id.to_string()),
            Event::WindowMoved { id, col, row } =>
                ("window_moved", format!("{id},{col},{row}")),
            Event::WindowPlaced { id, policy_matched, slot_assigned } =>
                ("window_placed", format!("{id},{policy_matched},{slot_assigned}")),
            Event::WindowFocus { id } =>
                ("window_focus", id.to_string()),
            Event::WindowTitle { id, title } =>
                ("window_title", format!("{id},{title}")),
            Event::WindowAppId { id, app_id } =>
                ("window_app_id", format!("{id},{app_id}")),
            Event::WindowStateChanged { id, state } =>
                ("window_state_changed", format!("{id},{state}")),
            Event::WindowSlotChanged { id, slot } =>
                ("window_slot_changed", format!("{id},{slot}")),
            Event::WindowSlotAdapted { id, requested, actual } =>
                ("window_slot_adapted", format!("{id},{requested},{actual}")),
            Event::WindowStateAdapted { id, requested_state, actual_state, actual_slot } =>
                ("window_state_adapted", format!("{id},{requested_state},{actual_state},{actual_slot}")),
            Event::WindowMoveSkipped { id, requested_col, requested_row, actual_col, actual_row } =>
                ("window_move_skipped", format!("{id},{requested_col},{requested_row},{actual_col},{actual_row}")),
            Event::WindowMoveRefused { id, requested_col, requested_row, reason } =>
                ("window_move_refused", format!("{id},{requested_col},{requested_row},{reason}")),
            Event::WindowStackChanged { slot, top_id, size } =>
                ("window_stack_changed", format!("{slot},{top_id},{size}")),
            Event::WindowStackReordered { slot, order_csv } =>
                ("window_stack_reordered", format!("{slot},{order_csv}")),
            Event::SubMapChanged { name } =>
                ("submap_changed", name.clone()),
            Event::ConfigReloaded { which } =>
                ("config_reloaded", which.clone()),
            Event::ConfigError { message } =>
                ("config_error", message.clone()),
            Event::ThemeReloaded =>
                ("theme_reloaded", String::new()),
            Event::ShaderLoaded { category, path } =>
                ("shader_loaded", format!("{category},{path}")),
            Event::ShaderError { category, path, log } =>
                ("shader_error", format!("{category},{path},{log}")),
            Event::OverviewOpened =>
                ("overview_opened", String::new()),
            Event::OverviewClosed =>
                ("overview_closed", String::new()),
            Event::ViewModeChanged { mode } =>
                ("view_mode_changed", mode.clone()),
            Event::MinimapToggled { visible } =>
                ("minimap_toggled", visible.to_string()),
            Event::MonitorAdded { name, width, height, scale } =>
                ("monitor_added", format!("{name},{width},{height},{scale}")),
            Event::MonitorRemoved { name } =>
                ("monitor_removed", name.clone()),
            Event::MonitorConfigChanged { name } =>
                ("monitor_config_changed", name.clone()),
            Event::KeyboardLayoutChanged { device, layout } =>
                ("keyboard_layout_changed", format!("{device},{layout}")),
            Event::IdleTimeout { after_seconds } =>
                ("idle", after_seconds.to_string()),
            Event::IdleResume { after_seconds } =>
                ("resume", after_seconds.to_string()),
            Event::Urgent { id } =>
                ("urgent", id.to_string()),
            Event::SessionLocked =>
                ("lock", String::new()),
            Event::SessionUnlocked =>
                ("unlock", String::new()),
            Event::SafeModeEntered { reason } =>
                ("safe_mode_entered", reason.clone()),
            Event::NotificationDaemonMissing =>
                ("notification_daemon_missing", String::new()),
            Event::PluginError { name, message } =>
                ("plugin_error", format!("{name},{message}")),
            Event::PluginLoaded { name } =>
                ("plugin_loaded", name.clone()),
            Event::WindowSizeConstraintFloat { id, slot, min_w, min_h, slot_w, slot_h } =>
                ("window_size_constraint_forced_float",
                    format!("{id},{slot},{min_w},{min_h},{slot_w},{slot_h}")),
        };
        format!("{name}>>{data}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::Event;

    fn parse(s: &str) -> (&str, &str) {
        let s = s.strip_suffix('\n').unwrap();
        let (name, data) = s.split_once(">>").unwrap();
        (name, data)
    }

    #[test]
    fn workspace_changed_wire() {
        let wire = Event::WorkspaceChanged { col: 1, row: 2 }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "workspace_changed");
        assert!(data.starts_with("1,2,"));
    }

    #[test]
    fn window_opened_wire() {
        let wire = Event::WindowOpened {
            id: 42,
            app_id: "kitty".into(),
            workspace_col: 0,
            workspace_row: 0,
            state: "tiled".into(),
            slot: "half-left".into(),
        }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "window_opened");
        assert!(data.contains("42"));
        assert!(data.contains("kitty"));
    }

    #[test]
    fn window_closed_wire() {
        let wire = Event::WindowClosed { id: 7 }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "window_closed");
        assert_eq!(data, "7");
    }

    #[test]
    fn overview_events_have_empty_data() {
        for ev in [Event::OverviewOpened, Event::OverviewClosed] {
            let wire = ev.format();
            let (_, data) = parse(&wire);
            assert_eq!(data, "");
        }
    }

    #[test]
    fn plugin_error_wire() {
        let wire = Event::PluginError {
            name: "myplugin".into(),
            message: "abi mismatch".into(),
        }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "plugin_error");
        assert!(data.contains("myplugin"));
        assert!(data.contains("abi mismatch"));
    }

    #[test]
    fn session_lock_unlock_wire() {
        assert_eq!(Event::SessionLocked.format(), "lock>>\n");
        assert_eq!(Event::SessionUnlocked.format(), "unlock>>\n");
    }

    #[test]
    fn wire_always_ends_with_newline() {
        let events = vec![
            Event::WorkspaceChanged { col: 0, row: 0 },
            Event::WindowFocus { id: 1 },
            Event::SubMapChanged { name: "resize".into() },
            Event::ConfigReloaded { which: "config.toml".into() },
        ];
        for ev in events {
            assert!(ev.format().ends_with('\n'), "missing newline in {:?}", ev);
        }
    }

    #[test]
    fn window_slot_adapted_wire() {
        let wire = Event::WindowSlotAdapted {
            id: 5,
            requested: "half-left".into(),
            actual: "quarter-bl".into(),
        }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "window_slot_adapted");
        assert!(data.contains("5"));
        assert!(data.contains("half-left"));
        assert!(data.contains("quarter-bl"));
    }

    #[test]
    fn window_state_adapted_wire() {
        let wire = Event::WindowStateAdapted {
            id: 9,
            requested_state: "fullscreen".into(),
            actual_state: "tiled".into(),
            actual_slot: "half-right".into(),
        }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "window_state_adapted");
        assert!(data.contains("9"));
        assert!(data.contains("fullscreen"));
        assert!(data.contains("half-right"));
    }

    #[test]
    fn window_stack_reordered_wire() {
        let wire = Event::WindowStackReordered {
            slot: "half-left".into(),
            order_csv: "3,1,2".into(),
        }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "window_stack_reordered");
        assert!(data.starts_with("half-left,"));
        assert!(data.contains("3,1,2"));
    }

    #[test]
    fn window_move_refused_wire() {
        let wire = Event::WindowMoveRefused {
            id: 7,
            requested_col: 2,
            requested_row: 1,
            reason: "tf-protected".into(),
        }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "window_move_refused");
        assert!(data.contains("7"));
        assert!(data.contains("tf-protected"));
    }

    #[test]
    fn notification_daemon_missing_wire() {
        let wire = Event::NotificationDaemonMissing.format();
        let (name, _data) = parse(&wire);
        assert_eq!(name, "notification_daemon_missing");
    }

    #[test]
    fn safe_mode_entered_wire() {
        let wire = Event::SafeModeEntered { reason: "3 crashes in 30s".into() }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "safe_mode_entered");
        assert!(data.contains("crashes"));
    }

    #[test]
    fn config_error_wire() {
        let wire = Event::ConfigError { message: "parse error on line 5".into() }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "config_error");
        assert!(data.contains("parse error"));
    }

    #[test]
    fn config_reloaded_wire() {
        let wire = Event::ConfigReloaded { which: "theme".into() }.format();
        let (name, data) = parse(&wire);
        assert_eq!(name, "config_reloaded");
        assert_eq!(data, "theme");
    }
}
