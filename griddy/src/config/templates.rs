use serde::{Deserialize, Serialize};

/// Top-level wrapper matching `[[template]]` entries in `workspace-templates.toml`.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WorkspaceTemplatesConfig {
    #[serde(default, rename = "template")]
    pub templates: Vec<TemplateConfig>,
}

/// One workspace template.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TemplateConfig {
    /// `[col, row]` workspace coordinates (0-based).
    pub cell: [u8; 2],
    /// Optional human-readable workspace label.
    pub name: Option<String>,
    /// Ordered list of windows to place in this workspace.
    #[serde(default, rename = "window")]
    pub windows: Vec<TemplateWindowConfig>,
}

/// One window entry inside a template.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TemplateWindowConfig {
    /// Slot to assign: "HalfLeft", "HalfRight", "QuarterTL", "QuarterTR",
    /// "QuarterBL", "QuarterBR", "Fullscreen", or "Floating".
    pub slot: String,
    /// Command to spawn if no matching window is already open.
    pub exec: String,
    /// Optional app_id glob — if a window matching this already exists on
    /// any workspace, move it here instead of spawning a new one.
    pub app_id: Option<String>,
}
