//! Session state persistence (§22.1).
//!
//! Saves window→workspace assignments on graceful shutdown and restores them
//! on startup by matching new windows to saved entries by `app_id` and `title`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionWindow {
    pub app_id: String,
    pub title: String,
    /// Slot name: "HalfLeft", "HalfRight", "QuarterTL", etc. — None if floating/fullscreen.
    pub slot: Option<String>,
    /// Window state: "Tiled", "Floating", "Fullscreen", "TotalFullscreen".
    pub state: String,
    pub workspace_col: u8,
    pub workspace_row: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub version: u32,
    pub windows: Vec<SessionWindow>,
}

// ─── Paths ────────────────────────────────────────────────────────────────────

pub fn state_dir() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
            PathBuf::from(home).join(".local/state")
        });
    base.join("griddy")
}

fn session_file() -> PathBuf {
    state_dir().join("session.json")
}

// ─── Load / Save ─────────────────────────────────────────────────────────────

pub fn load() -> Option<SessionState> {
    let path = session_file();
    let content = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<SessionState>(&content) {
        Ok(s) => {
            tracing::info!(
                windows = s.windows.len(),
                "Loaded session from {}",
                path.display()
            );
            Some(s)
        }
        Err(e) => {
            tracing::warn!("Failed to parse session file: {e}");
            None
        }
    }
}

pub fn save(state: &crate::state::GlobalState) {
    let dir = state_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("session: cannot create state dir {}: {e}", dir.display());
        return;
    }

    let mut windows: Vec<SessionWindow> = Vec::new();
    for w in state.grid.windows.values() {
        // Skip special-workspace windows (sentinel (MAX,MAX)) and swallowed windows.
        if w.workspace == (u8::MAX, u8::MAX) || w.is_swallowed {
            continue;
        }
        let slot = w.current_slot.map(|s| format!("{s:?}"));
        let wstate = format!("{:?}", w.current_state);
        windows.push(SessionWindow {
            app_id: w.app_id.clone(),
            title: w.title.clone(),
            slot,
            state: wstate,
            workspace_col: w.workspace.0,
            workspace_row: w.workspace.1,
        });
    }

    let session = SessionState {
        version: 1,
        windows,
    };
    let path = session_file();
    match serde_json::to_string_pretty(&session) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!("session: write failed {}: {e}", path.display());
            } else {
                tracing::info!(
                    windows = session.windows.len(),
                    "Session saved to {}",
                    path.display()
                );
            }
        }
        Err(e) => tracing::warn!("session: serialize failed: {e}"),
    }
}

pub fn clear() {
    let path = session_file();
    let _ = std::fs::remove_file(&path);
    tracing::info!("Session cleared");
}

// ─── Restore hint lookup ──────────────────────────────────────────────────────

impl SessionState {
    /// Find the saved entry best matching the given `app_id` + `title`.
    ///
    /// Matching priority: exact app_id + exact title → exact app_id only → none.
    /// Consumed entries are removed so each restore is applied at most once.
    pub fn take_hint(&mut self, app_id: &str, title: &str) -> Option<SessionWindow> {
        // Try exact match on both fields first.
        if let Some(idx) = self
            .windows
            .iter()
            .position(|w| w.app_id == app_id && w.title == title)
        {
            return Some(self.windows.remove(idx));
        }
        // Fall back to app_id-only match.
        if let Some(idx) = self.windows.iter().position(|w| w.app_id == app_id) {
            return Some(self.windows.remove(idx));
        }
        None
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(entries: Vec<(&str, &str, u8, u8)>) -> SessionState {
        SessionState {
            version: 1,
            windows: entries
                .into_iter()
                .map(|(app_id, title, col, row)| SessionWindow {
                    app_id: app_id.into(),
                    title: title.into(),
                    slot: Some("HalfLeft".into()),
                    state: "Tiled".into(),
                    workspace_col: col,
                    workspace_row: row,
                })
                .collect(),
        }
    }

    #[test]
    fn take_hint_exact_match() {
        let mut s = make_state(vec![
            ("kitty", "Terminal", 0, 0),
            ("firefox", "Browser", 1, 0),
        ]);
        let h = s.take_hint("kitty", "Terminal").unwrap();
        assert_eq!(h.workspace_col, 0);
        assert_eq!(s.windows.len(), 1);
    }

    #[test]
    fn take_hint_app_id_fallback() {
        let mut s = make_state(vec![("kitty", "Terminal", 0, 0)]);
        let h = s.take_hint("kitty", "Other Title").unwrap();
        assert_eq!(h.workspace_col, 0);
    }

    #[test]
    fn take_hint_no_match() {
        let mut s = make_state(vec![("kitty", "Terminal", 0, 0)]);
        assert!(s.take_hint("firefox", "Browser").is_none());
    }

    #[test]
    fn take_hint_prefers_exact_over_appid() {
        let mut s = make_state(vec![
            ("kitty", "Terminal A", 0, 0),
            ("kitty", "Terminal B", 1, 0),
        ]);
        // Exact match for "Terminal B" should win over first app_id match.
        let h = s.take_hint("kitty", "Terminal B").unwrap();
        assert_eq!(h.workspace_col, 1);
    }

    #[test]
    fn take_hint_consumed_only_once() {
        let mut s = make_state(vec![("kitty", "Terminal", 0, 0)]);
        let first = s.take_hint("kitty", "Terminal");
        let second = s.take_hint("kitty", "Terminal");
        assert!(first.is_some());
        assert!(second.is_none());
    }

    #[test]
    fn session_round_trip_json() {
        let state = SessionState {
            version: 1,
            windows: vec![SessionWindow {
                app_id: "kitty".into(),
                title: "Terminal".into(),
                slot: Some("HalfLeft".into()),
                state: "Tiled".into(),
                workspace_col: 0,
                workspace_row: 1,
            }],
        };
        let json = serde_json::to_string(&state).unwrap();
        let restored: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.windows.len(), 1);
        assert_eq!(restored.windows[0].app_id, "kitty");
        assert_eq!(restored.windows[0].workspace_row, 1);
    }
}
