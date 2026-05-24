pub mod monitors;
pub mod rules;
pub mod templates;
pub mod theme;
pub mod types;

#[allow(unused_imports)]
pub use monitors::{load_monitors, MonitorConfig, MonitorDefaults, MonitorsConfig, OutputTransform, VrrPolicy};
pub use templates::WorkspaceTemplatesConfig;
pub use theme::*;
pub use types::*;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Resolve config file path in XDG order.
pub fn config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("GRIDDY_CONFIG") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let path = PathBuf::from(xdg).join("griddy/config.toml");
        if path.exists() {
            return Some(path);
        }
    }
    if let Some(home) = home_config_dir() {
        let path = home.join("griddy/config.toml");
        if path.exists() {
            return Some(path);
        }
    }
    let system = PathBuf::from("/etc/griddy/config.toml");
    if system.exists() {
        return Some(system);
    }
    None
}

fn home_config_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config"))
}

/// Load and parse config from file. Returns default config if file doesn't exist.
pub fn load_config(path: Option<&Path>) -> Result<Config> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => match config_path() {
            Some(p) => p,
            None => {
                tracing::info!("No config file found, using defaults");
                return Ok(Config::default());
            }
        },
    };

    tracing::info!("Loading config from {}", path.display());
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;

    let mut config: Config = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config: {}", path.display()))?;

    let base_dir = path.parent().unwrap_or(Path::new("."));

    // Process explicit imports.
    for import in &config.imports.clone() {
        let import_path = base_dir.join(import);
        if import_path.exists() {
            tracing::debug!("Loading import: {}", import_path.display());
            let import_content = std::fs::read_to_string(&import_path)
                .with_context(|| format!("Failed to read import: {}", import_path.display()))?;
            merge_config(&mut config, &import_content, &import_path)?;
        } else {
            tracing::warn!("Import not found: {}", import_path.display());
        }
    }

    // Auto-load theme.toml from same directory.
    let theme_path = base_dir.join("theme.toml");
    if theme_path.exists() {
        tracing::debug!("Loading theme from {}", theme_path.display());
        match load_theme_file(&theme_path) {
            Ok(theme) => config.theme = theme,
            Err(e) => tracing::warn!("Failed to load theme.toml (using defaults): {e}"),
        }
    }

    // Auto-load rules.toml from same directory.
    let rules_path = base_dir.join("rules.toml");
    if rules_path.exists() {
        tracing::debug!("Loading rules from {}", rules_path.display());
        match load_rules_file(&rules_path) {
            Ok(mut extra_rules) => config.rules.append(&mut extra_rules),
            Err(e) => tracing::warn!("Failed to load rules.toml: {e}"),
        }
    }

    // Auto-load keybinds.toml from same directory.
    let keybinds_path = base_dir.join("keybinds.toml");
    if keybinds_path.exists() {
        tracing::debug!("Loading keybinds from {}", keybinds_path.display());
        match load_keybinds_file(&keybinds_path) {
            Ok(mut extra_binds) => config.binds.append(&mut extra_binds),
            Err(e) => tracing::warn!("Failed to load keybinds.toml: {e}"),
        }
    }

    tracing::debug!(
        rules = config.rules.len(),
        binds = config.binds.len(),
        submaps = config.submaps.len(),
        "Config loaded"
    );
    Ok(config)
}

pub fn load_theme_file(path: &Path) -> Result<ThemeConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read theme: {}", path.display()))?;

    // Parse user TOML as a value first so we can check `import`.
    let mut user_val: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse theme: {}", path.display()))?;

    // Resolve `import = "<preset-name>"` before deserialising.
    let import_name = user_val
        .get("import")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned());

    if let Some(name) = import_name {
        if let Some(preset_path) = resolve_theme_preset(&name) {
            tracing::debug!("Loading theme preset '{}' from {}", name, preset_path.display());
            match std::fs::read_to_string(&preset_path) {
                Ok(preset_content) => {
                    match toml::from_str::<toml::Value>(&preset_content) {
                        Ok(mut base_val) => {
                            // Overlay user's values on top of the preset.
                            merge_toml(&mut base_val, user_val);
                            user_val = base_val;
                        }
                        Err(e) => tracing::warn!("Failed to parse theme preset '{}': {e}", name),
                    }
                }
                Err(e) => tracing::warn!("Failed to read theme preset '{}': {e}", name),
            }
        } else {
            tracing::warn!("Theme preset '{}' not found in search paths", name);
        }
    }

    user_val
        .try_into()
        .with_context(|| format!("Failed to deserialise theme: {}", path.display()))
}

/// Search for a named theme preset file.
///
/// Search order:
///   1. `$XDG_CONFIG_HOME/griddy/themes/<name>.toml`
///   2. `~/.config/griddy/themes/<name>.toml`
///   3. `/usr/share/griddy/themes/<name>.toml`
fn resolve_theme_preset(name: &str) -> Option<PathBuf> {
    let file = format!("{name}.toml");

    // Sanitise: reject names that try to escape the directory.
    if name.contains('/') || name.contains("..") {
        tracing::warn!("Rejecting unsafe theme preset name: '{name}'");
        return None;
    }

    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            v.push(PathBuf::from(xdg).join("griddy/themes").join(&file));
        }
        if let Some(home_cfg) = home_config_dir() {
            v.push(home_cfg.join("griddy/themes").join(&file));
        }
        v.push(PathBuf::from("/usr/share/griddy/themes").join(&file));
        v
    };

    candidates.into_iter().find(|p| p.exists())
}

/// Rules file format: `[[rule]]` entries at top level.
fn load_rules_file(path: &Path) -> Result<Vec<rules::Rule>> {
    #[derive(serde::Deserialize)]
    struct RulesFile {
        #[serde(default, rename = "rule")]
        rules: Vec<rules::Rule>,
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read rules: {}", path.display()))?;
    let file: RulesFile = toml::from_str(&content)
        .with_context(|| format!("Failed to parse rules: {}", path.display()))?;
    Ok(file.rules)
}

/// Load workspace-templates.toml from the config directory.  Returns an empty
/// config (no templates) if the file doesn't exist or fails to parse.
pub fn load_templates(config_dir: &Path) -> WorkspaceTemplatesConfig {
    let path = config_dir.join("workspace-templates.toml");
    if !path.exists() {
        return WorkspaceTemplatesConfig::default();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to read workspace-templates.toml: {e}");
            return WorkspaceTemplatesConfig::default();
        }
    };
    match toml::from_str(&content) {
        Ok(cfg) => {
            tracing::debug!("Loaded workspace-templates.toml");
            cfg
        }
        Err(e) => {
            tracing::warn!("Failed to parse workspace-templates.toml: {e}");
            WorkspaceTemplatesConfig::default()
        }
    }
}

/// Keybinds file format: `[[bind]]` entries at top level.
fn load_keybinds_file(path: &Path) -> Result<Vec<BindConfig>> {
    #[derive(serde::Deserialize)]
    struct KeybindsFile {
        #[serde(default, rename = "bind")]
        binds: Vec<BindConfig>,
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read keybinds: {}", path.display()))?;
    let file: KeybindsFile = toml::from_str(&content)
        .with_context(|| format!("Failed to parse keybinds: {}", path.display()))?;
    Ok(file.binds)
}

fn merge_config(base: &mut Config, content: &str, path: &Path) -> Result<()> {
    let overlay: toml::Value = toml::from_str(content)
        .with_context(|| format!("Failed to parse import: {}", path.display()))?;
    let mut base_value = toml::Value::try_from(base.clone())
        .context("Failed to serialize base config")?;
    merge_toml(&mut base_value, overlay);
    *base = base_value
        .try_into()
        .with_context(|| format!("Failed to deserialize merged config from {}", path.display()))?;
    Ok(())
}

fn merge_toml(base: &mut toml::Value, overlay: toml::Value) {
    if let (toml::Value::Table(b), toml::Value::Table(o)) = (base, overlay) {
        for (k, v) in o {
            if let Some(bv) = b.get_mut(&k) {
                merge_toml(bv, v);
            } else {
                b.insert(k, v);
            }
        }
    }
}

/// Apply [env] block: set environment variables for the compositor session.
///
/// # Safety
/// Called before spawning threads; safe in a single-threaded context.
pub fn apply_env(config: &Config) {
    unsafe {
        std::env::set_var("XDG_CURRENT_DESKTOP", "GriddyWM");
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
        // Export cursor theme/size so child processes (GTK, Qt, XWayland) pick them up.
        std::env::set_var("XCURSOR_THEME", &config.theme.cursor.theme);
        std::env::set_var("XCURSOR_SIZE",   config.theme.cursor.size.to_string());
        for (key, val) in &config.env {
            tracing::debug!("Setting env: {}={}", key, val);
            std::env::set_var(key, val);
        }
    }
}
