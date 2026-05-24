//! Window rules — §10. Match windows by app_id/title/workspace and apply
//! property overrides before the placement policy runs.

use regex::Regex;
use serde::{Deserialize, Serialize};

// ─── Rule ─────────────────────────────────────────────────────────────────────

/// A single [[rule]] entry.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Rule {
    /// Match criteria (all specified criteria must match).
    #[serde(rename = "match", default)]
    pub match_: RuleMatch,
    /// Properties applied when the rule matches.
    #[serde(default)]
    pub action: RuleAction,
}

impl Rule {
    /// Return true if this rule matches the given window properties.
    /// All specified match criteria must match (AND semantics).
    pub fn matches(
        &self,
        app_id: &str,
        title: &str,
        is_xwayland: bool,
        workspace: Option<(u8, u8)>,
        fullscreen_request: bool,
    ) -> bool {
        let m = &self.match_;
        if let Some(pat) = &m.app_id {
            if !glob_match(pat, app_id) {
                return false;
            }
        }
        if let Some(pat) = &m.title {
            if !glob_match(pat, title) {
                return false;
            }
        }
        if let Some(pat) = &m.title_regex {
            match Regex::new(pat) {
                Ok(re) => {
                    if !re.is_match(title) {
                        return false;
                    }
                }
                Err(e) => {
                    tracing::warn!(pattern = %pat, error = %e, "Invalid title_regex in rule — skipping rule");
                    return false;
                }
            }
        }
        if let Some([col, row]) = m.workspace {
            match workspace {
                Some((wc, wr)) => {
                    if col != wc || row != wr {
                        return false;
                    }
                }
                None => return false,
            }
        }
        if let Some(req) = m.fullscreen_request {
            if fullscreen_request != req {
                return false;
            }
        }
        if let Some(is_x) = m.is_xwayland {
            if is_xwayland != is_x {
                return false;
            }
        }
        true
    }
}

// ─── Match criteria ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RuleMatch {
    /// Glob pattern matched against the window's app_id.
    #[serde(default)]
    pub app_id: Option<String>,
    /// Glob pattern matched against the window's title.
    #[serde(default)]
    pub title: Option<String>,
    /// POSIX extended regex matched against the window's title.
    #[serde(default)]
    pub title_regex: Option<String>,
    /// Match only windows on this workspace [col, row].
    #[serde(default)]
    pub workspace: Option<[u8; 2]>,
    /// Match only when the window requests fullscreen at creation.
    #[serde(default)]
    pub fullscreen_request: Option<bool>,
    /// Match only XWayland windows (true) or only Wayland-native windows (false).
    #[serde(default)]
    pub is_xwayland: Option<bool>,
}

// ─── Rule actions ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuleAction {
    /// Override initial slot: "half-left" | "half-right" | "quarter-tl" | ...
    #[serde(default)]
    pub slot: Option<String>,
    /// Override initial state: "tiled" | "floating" | "fullscreen" | "total-fullscreen"
    #[serde(default)]
    pub state: Option<String>,
    /// Force placement on this workspace [col, row].
    #[serde(default)]
    pub workspace: Option<[u8; 2]>,
    /// Initial floating geometry [x, y, w, h].
    #[serde(default)]
    pub floating_geom: Option<[i32; 4]>,
    /// Window opacity override (0.0–1.0).
    #[serde(default)]
    pub opacity: Option<f32>,
    /// Don't give focus when this window opens.
    #[serde(default)]
    pub no_focus: bool,
    /// Pin to all workspaces as floating (visible on every workspace).
    #[serde(default)]
    pub pin: bool,
    /// Alias for pin: window follows you to every workspace.
    #[serde(default)]
    pub sticky: bool,
    /// Skip all open/close/move animations for this window.
    #[serde(default)]
    pub no_animations: bool,
    /// Skip drop shadow rendering for this window.
    #[serde(default)]
    pub no_shadow: bool,
    /// Skip background blur for this window.
    #[serde(default)]
    pub no_blur: bool,
    /// Render this floating window above TotalFullscreen on the same workspace (default true).
    #[serde(default = "default_above_tf")]
    pub above_total_fullscreen: bool,
    /// Path to a per-window GLSL fragment shader.
    #[serde(default)]
    pub shader: Option<String>,
    /// Custom border color override (hex: "#rrggbb" or "#rrggbbaa").
    #[serde(default)]
    pub border_color: Option<String>,
    /// Decoration mode: "server" | "client" | "none"
    #[serde(default)]
    pub decorations: Option<String>,
    /// Min-size overflow policy override: "float" | "clip" | "ignore"
    #[serde(default)]
    pub min_size_overflow: Option<String>,
    /// Allow screen tearing for this window (useful for games).
    #[serde(default)]
    pub allow_tearing: bool,
    /// Glob matched against the swallower's app_id (§22.4).
    /// When set, windows whose parent process owns a window matching this glob
    /// will take that window's slot; the parent hides until the child closes.
    #[serde(default)]
    pub swallow: Option<String>,
    /// Allow this window to steal focus even when focus_steal_prevention is on (§22.5).
    #[serde(default)]
    pub steal_focus: bool,
}

impl Default for RuleAction {
    fn default() -> Self {
        Self {
            slot: None,
            state: None,
            workspace: None,
            floating_geom: None,
            opacity: None,
            no_focus: false,
            pin: false,
            sticky: false,
            no_animations: false,
            no_shadow: false,
            no_blur: false,
            above_total_fullscreen: true,
            shader: None,
            border_color: None,
            decorations: None,
            min_size_overflow: None,
            allow_tearing: false,
            swallow: None,
            steal_focus: false,
        }
    }
}

fn default_above_tf() -> bool { true }

// ─── Glob matching ────────────────────────────────────────────────────────────

/// Simple glob matcher supporting `*` (any sequence) and `?` (one char).
fn glob_match(pat: &str, s: &str) -> bool {
    glob_bytes(pat.as_bytes(), s.as_bytes())
}

/// Public re-export for use in the swallowing logic.
pub fn glob_match_pub(pat: &str, s: &str) -> bool {
    glob_bytes(pat.as_bytes(), s.as_bytes())
}

fn glob_bytes(pat: &[u8], s: &[u8]) -> bool {
    match (pat.first(), s.first()) {
        (None, None) => true,
        (Some(&b'*'), _) => {
            glob_bytes(&pat[1..], s) || (!s.is_empty() && glob_bytes(pat, &s[1..]))
        }
        (Some(&b'?'), Some(_)) => glob_bytes(&pat[1..], &s[1..]),
        (Some(p), Some(c)) if p == c => glob_bytes(&pat[1..], &s[1..]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    fn make_rule(app_id: Option<&str>, title: Option<&str>) -> super::Rule {
        let mut r = super::Rule::default();
        r.match_.app_id = app_id.map(|s| s.to_owned());
        r.match_.title  = title.map(|s| s.to_owned());
        r
    }

    #[test]
    fn exact() {
        assert!(glob_match("kitty", "kitty"));
        assert!(!glob_match("kitty", "alacritty"));
    }

    #[test]
    fn star_prefix() {
        assert!(glob_match("*terminal*", "foot-terminal-app"));
        assert!(glob_match("org.gnome.*", "org.gnome.Files"));
    }

    #[test]
    fn question_mark() {
        assert!(glob_match("k?tty", "kitty"));
        assert!(!glob_match("k?tty", "ktty"));
    }

    #[test]
    fn matches_no_criteria_always_true() {
        let r = make_rule(None, None);
        assert!(r.matches("anything", "anything", false, None, false));
    }

    #[test]
    fn matches_app_id() {
        let r = make_rule(Some("kitty"), None);
        assert!( r.matches("kitty", "", false, None, false));
        assert!(!r.matches("alacritty", "", false, None, false));
    }

    #[test]
    fn matches_workspace() {
        let mut r = super::Rule::default();
        r.match_.workspace = Some([1, 2]);
        assert!( r.matches("", "", false, Some((1, 2)), false));
        assert!(!r.matches("", "", false, Some((0, 0)), false));
        assert!(!r.matches("", "", false, None, false));
    }

    #[test]
    fn matches_title_regex() {
        let mut r = super::Rule::default();
        r.match_.title_regex = Some("Friends List".to_owned());
        assert!( r.matches("Steam", "Friends List", false, None, false));
        assert!(!r.matches("Steam", "Library", false, None, false));
    }

    #[test]
    fn matches_is_xwayland() {
        let mut r = super::Rule::default();
        r.match_.is_xwayland = Some(true);
        assert!( r.matches("", "", true,  None, false));
        assert!(!r.matches("", "", false, None, false));
    }
}
