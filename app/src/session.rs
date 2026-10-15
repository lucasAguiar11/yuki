//! UI state that outlives a run, in `session.yaml` (data dir, machine-local,
//! never in a workspace's git): window, panel visibility and sizes, and per
//! workspace the open tabs, collapsed folders and env. Kept as a global so
//! nested views (the request/response split) can read and update it; written
//! on quit with the other flushes.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::{backend, workspace::Yuki};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Session {
    /// The workspace open at quit.
    pub workspace: Option<String>,
    /// Window x, y, width, height.
    pub window: Option<[f32; 4]>,
    pub sidebar: bool,
    pub vars_panel: bool,
    pub git_panel: bool,
    pub sidebar_width: f32,
    /// Variables / git panel.
    pub panel_width: f32,
    /// Request editor above the response, or beside it.
    pub editor_height: f32,
    pub editor_width: f32,
    pub workspaces: HashMap<String, WsSession>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            workspace: None,
            window: None,
            sidebar: true,
            vars_panel: false,
            git_panel: false,
            sidebar_width: 240.,
            panel_width: 300.,
            editor_height: 240.,
            editor_width: 520.,
            workspaces: HashMap::new(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WsSession {
    /// Request ids, in tab order; gone ones are skipped on restore.
    pub tabs: Vec<String>,
    pub active: Option<String>,
    pub collapsed: HashSet<String>,
    pub env: Option<String>,
}

impl Global for Session {}

fn path() -> PathBuf {
    backend::data_dir().join("session.yaml")
}

impl Session {
    /// Startup: read the file into the global (broken or missing = defaults)
    /// and return where the window opens.
    pub fn init(cx: &mut App) -> WindowBounds {
        let s: Session = std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_yaml::from_str(&t).ok())
            .unwrap_or_default();
        let saved = s
            .window
            .map(|[x, y, w, h]| Bounds::new(point(px(x), px(y)), size(px(w), px(h))))
            // Its display may be gone since: never open off-screen.
            .filter(|b| cx.displays().iter().any(|d| d.bounds().intersects(b)));
        cx.set_global(s);
        match saved {
            Some(b) => WindowBounds::Windowed(b),
            None => WindowBounds::centered(size(px(1280.), px(800.)), cx),
        }
    }

    /// Keep the global's window bounds current (moves and resizes).
    pub fn track_window(window: &mut Window, cx: &mut Context<Yuki>) {
        cx.observe_window_bounds(window, |_, window, cx| {
            let b = window.window_bounds().get_bounds();
            cx.global_mut::<Session>().window = Some([
                b.origin.x.as_f32(),
                b.origin.y.as_f32(),
                b.size.width.as_f32(),
                b.size.height.as_f32(),
            ]);
        })
        .detach();
    }
}

impl Yuki {
    fn ws_session(&self, cx: &App) -> WsSession {
        let id = |t: &Entity<crate::editor::RequestEditor>| t.read(cx).saved.id.clone();
        WsSession {
            tabs: self.tabs.iter().map(id).collect(),
            active: self.tabs.get(self.active).map(id),
            collapsed: self.collapsed.clone(),
            env: self.env_id(cx),
        }
    }

    /// Before leaving the open workspace: remember its tabs, folders and env.
    pub(crate) fn remember_ws(&self, cx: &mut App) {
        let Some(id) = self.ws.as_ref().map(|w| w.id.clone()) else {
            return;
        };
        let ws = self.ws_session(cx);
        cx.global_mut::<Session>().workspaces.insert(id, ws);
    }

    /// Quit: write the session synchronously.
    pub(crate) fn save_session(&self, cx: &App) {
        let mut s = cx.global::<Session>().clone();
        if let Some(ws) = &self.ws {
            s.workspace = Some(ws.id.clone());
            s.workspaces.insert(ws.id.clone(), self.ws_session(cx));
        }
        (s.sidebar, s.vars_panel, s.git_panel) = (self.sidebar, self.vars_panel, self.git_panel);
        let written = serde_yaml::to_string(&s)
            .map_err(|e| e.to_string())
            .and_then(|t| yuki_storage::write_atomic(path(), t).map_err(|e| e.to_string()));
        if let Err(e) = written {
            eprintln!("[yuki] failed to save session: {e}");
        }
    }

    /// Open `ids` as tabs (missing ones skipped), `active` selected.
    pub(crate) fn open_tabs(
        &mut self,
        ids: &[String],
        active: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for id in ids {
            self.open_request(id, window, cx);
        }
        let ix = active.and_then(|a| self.tabs.iter().position(|t| t.read(cx).saved.id == a));
        self.active = ix.unwrap_or(0);
        self.focus_active(window, cx);
    }
}
