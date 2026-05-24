//! `monitors.toml` — per-output configuration (§8.6).
//!
//! Loaded from the same directory as `config.toml`.  Applied during DRM/KMS
//! backend initialisation; informational in the winit dev backend.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Transform (rotation + flip) for a monitor output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputTransform {
    #[default]
    Normal,
    #[serde(rename = "90")]
    Rotate90,
    #[serde(rename = "180")]
    Rotate180,
    #[serde(rename = "270")]
    Rotate270,
    Flipped,
    #[serde(rename = "flipped-90")]
    Flipped90,
    #[serde(rename = "flipped-180")]
    Flipped180,
    #[serde(rename = "flipped-270")]
    Flipped270,
}

/// VRR / adaptive-sync policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VrrPolicy {
    #[default]
    Off,
    On,
    /// Enable only when a window is in fullscreen state.
    Fullscreen,
}

/// A single `[[monitor]]` entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MonitorConfig {
    /// Connector name as reported by the DRM layer (e.g. "DP-1", "HDMI-A-1").
    pub name: String,

    /// Desired mode.  Accepted formats:
    /// - `"preferred"` — let the driver choose the highest-resolution preferred mode
    /// - `"WxH"` — e.g. `"1920x1080"`
    /// - `"WxH@Hz"` — e.g. `"2560x1440@165"`
    #[serde(default = "default_mode")]
    pub mode: String,

    /// Position in the compositor's global logical coordinate space `[x, y]`.
    /// When set to `null` / omitted, the compositor places the output automatically
    /// (right of the last configured output).
    #[serde(default)]
    pub position: Option<[i32; 2]>,

    /// Fractional output scale factor.
    #[serde(default = "default_scale")]
    pub scale: f64,

    /// Output transform (rotation / flip).
    #[serde(default)]
    pub transform: OutputTransform,

    /// VRR / adaptive-sync policy.
    #[serde(default)]
    pub vrr: VrrPolicy,

    /// Whether VRR hardware adaptive-sync is enabled (alias for `vrr`).
    #[serde(default)]
    pub adaptive_sync: bool,

    /// Bit depth hint: 8 or 10 (HDR-capable path required for 10).
    #[serde(default = "default_bit_depth")]
    pub bit_depth: u8,

    /// ICC colour profile path, or `"auto"` / `"sRGB"` / `"rec2020"`.
    #[serde(default = "default_color_profile")]
    pub color_profile: String,

    /// Enable HDR (experimental).
    #[serde(default)]
    pub hdr: bool,

    /// Allow tearing when a window opts in via `wp_tearing_control_v1`.
    #[serde(default)]
    pub tearing: bool,

    /// Whether this output is active.
    #[serde(default = "bool_true")]
    pub enabled: bool,

    /// Per-output grid columns (overrides the global `grid.cols`).
    #[serde(default)]
    pub grid_cols: Option<u8>,

    /// Per-output grid rows (overrides the global `grid.rows`).
    #[serde(default)]
    pub grid_rows: Option<u8>,

    /// Whether this monitor participates in workspace-sync navigation.
    #[serde(default = "bool_true")]
    pub workspace_sync: bool,
}

fn default_mode() -> String {
    "preferred".into()
}

fn default_scale() -> f64 {
    1.0
}

fn default_bit_depth() -> u8 {
    8
}

fn default_color_profile() -> String {
    "auto".into()
}

fn bool_true() -> bool {
    true
}

/// Default monitor settings applied to any connector not explicitly listed.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MonitorDefaults {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// `"auto"` arranges outputs in a horizontal row.
    #[serde(default = "default_position")]
    pub position: String,
}

fn default_position() -> String {
    "auto".into()
}

impl Default for MonitorDefaults {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            scale: default_scale(),
            position: default_position(),
        }
    }
}

/// Top-level structure of `monitors.toml`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MonitorsConfig {
    #[serde(default, rename = "monitor")]
    pub monitors: Vec<MonitorConfig>,

    #[serde(default)]
    pub default: MonitorDefaults,
}

impl MonitorsConfig {
    /// Find the config entry for a connector by name, or return `None` if no
    /// explicit config was specified for it.
    pub fn for_connector(&self, name: &str) -> Option<&MonitorConfig> {
        self.monitors.iter().find(|m| m.name == name)
    }
}

/// Load `monitors.toml` from `config_dir`.  Returns an empty/default config
/// (no entries, default defaults) if the file does not exist.
pub fn load_monitors(config_dir: &Path) -> MonitorsConfig {
    let path = config_dir.join("monitors.toml");
    if !path.exists() {
        return MonitorsConfig::default();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to read monitors.toml: {e}");
            return MonitorsConfig::default();
        }
    };
    match toml::from_str::<MonitorsConfig>(&content) {
        Ok(cfg) => {
            tracing::debug!(monitors = ?cfg.monitors.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), "Loaded monitors.toml");
            cfg
        }
        Err(e) => {
            tracing::warn!("Failed to parse monitors.toml: {e}");
            MonitorsConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> MonitorsConfig {
        toml::from_str(s).expect("parse failed")
    }

    #[test]
    fn empty_file_gives_defaults() {
        let cfg = parse("");
        assert!(cfg.monitors.is_empty());
        assert_eq!(cfg.default.scale, 1.0);
    }

    #[test]
    fn single_monitor_entry() {
        let cfg = parse(r#"
[[monitor]]
name = "DP-1"
mode = "2560x1440@165"
scale = 1.0
position = [0, 0]
"#);
        assert_eq!(cfg.monitors.len(), 1);
        let m = &cfg.monitors[0];
        assert_eq!(m.name, "DP-1");
        assert_eq!(m.mode, "2560x1440@165");
        assert_eq!(m.position, Some([0, 0]));
    }

    #[test]
    fn default_mode_is_preferred() {
        let cfg = parse(r#"
[[monitor]]
name = "HDMI-A-1"
"#);
        assert_eq!(cfg.monitors[0].mode, "preferred");
    }

    #[test]
    fn for_connector_finds_by_name() {
        let cfg = parse(r#"
[[monitor]]
name = "eDP-1"
scale = 2.0

[[monitor]]
name = "DP-2"
scale = 1.0
"#);
        assert_eq!(cfg.for_connector("eDP-1").unwrap().scale, 2.0);
        assert_eq!(cfg.for_connector("DP-2").unwrap().scale, 1.0);
        assert!(cfg.for_connector("HDMI-A-1").is_none());
    }

    #[test]
    fn vrr_policy_parses() {
        let cfg = parse(r#"
[[monitor]]
name = "DP-1"
vrr = "fullscreen"
"#);
        assert_eq!(cfg.monitors[0].vrr, VrrPolicy::Fullscreen);
    }

    #[test]
    fn transform_parses() {
        let cfg = parse(r#"
[[monitor]]
name = "DP-1"
transform = "90"
"#);
        assert_eq!(cfg.monitors[0].transform, OutputTransform::Rotate90);
    }
}
