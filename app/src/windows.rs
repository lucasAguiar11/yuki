//! Several windows, one workspace each (a `Yuki` root per window). A workspace
//! lives in one window at a time: opening it again brings that window forward,
//! so two windows never edit the same files. Quit flushes every window, then
//! writes the session once.

use std::sync::Arc;

use gpui_kit::component::TitleBar;
use gpui_kit::*;
use tokio::sync::{broadcast, watch};

use crate::{
    backend::Backend,
    mcp::{Edit, Link},
    session::Session,
    workspace::Yuki,
};

pub struct Windows {
    backend: Arc<Backend>,
    edits: broadcast::Sender<Edit>,
    clients: watch::Receiver<usize>,
    open: Vec<(AnyWindowHandle, WeakEntity<Yuki>)>,
}

impl Global for Windows {}

pub fn init(
    backend: Arc<Backend>,
    edits: broadcast::Sender<Edit>,
    clients: watch::Receiver<usize>,
    cx: &mut App,
) {
    cx.set_global(Windows {
        backend,
        edits,
        clients,
        open: vec![],
    });
}

/// Reload workspace `ws` from disk in the window that shows it, the same way
/// an agent edit does.
pub fn reload(ws: String, cx: &App) {
    // No window subscribed: nothing shows it.
    let _ = cx.global::<Windows>().edits.send(Edit::Items(ws));
}

/// A window on workspace `ws` (`None`: the one from the last session).
pub fn open(ws: Option<String>, bounds: WindowBounds, cx: &mut App) {
    let w = cx.global::<Windows>();
    let (backend, link) = (
        w.backend.clone(),
        Link {
            edits: w.edits.subscribe(),
            clients: w.clients.clone(),
        },
    );
    let options = WindowOptions {
        window_bounds: Some(bounds),
        ..TitleBar::window_options()
    };
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| Yuki::new(backend, link, ws, window, cx))
    });
    match opened {
        Ok((handle, yuki)) => {
            let open = &mut cx.global_mut::<Windows>().open;
            open.retain(|(_, y)| y.upgrade().is_some());
            open.push((handle, yuki.downgrade()));
        }
        Err(e) => eprintln!("[yuki] failed to open a window: {e}"),
    }
}

/// `ws` in a new window, cascaded from the current one; or the window that
/// already shows it, brought forward.
pub fn open_new(ws: String, cx: &mut App) {
    if activate(&ws, None, cx) {
        return;
    }
    let current = cx
        .active_window()
        .and_then(|h| h.update(cx, |_, w, _| w.window_bounds().get_bounds()).ok());
    let bounds = match current {
        Some(b) => WindowBounds::Windowed(Bounds::new(b.origin + point(px(30.), px(30.)), b.size)),
        None => WindowBounds::centered(size(px(1280.), px(800.)), cx),
    };
    open(Some(ws), bounds, cx);
}

/// Bring forward the window showing `ws`, other than `except`. `false` when
/// none does. Skips `except` without reading it: it may be mid-update.
pub fn activate(ws: &str, except: Option<EntityId>, cx: &mut App) -> bool {
    let found = cx.global::<Windows>().open.iter().find_map(|(h, y)| {
        let y = y.upgrade().filter(|y| Some(y.entity_id()) != except)?;
        let shows = y.read(cx).ws.as_ref().is_some_and(|w| w.id == ws);
        shows.then_some(*h)
    });
    match found {
        Some(h) => h.update(cx, |_, w, _| w.activate_window()).is_ok(),
        None => false,
    }
}

fn alive(cx: &App) -> Vec<Entity<Yuki>> {
    let open = &cx.global::<Windows>().open;
    open.iter().filter_map(|(_, y)| y.upgrade()).collect()
}

/// Every window's tabs into the session, `last` written last so its workspace
/// and panels are what the next launch opens. Never call it while a `Yuki` is
/// being updated (it reads them all).
pub fn save_session(last: Option<EntityId>, cx: &mut App) {
    let mut yukis = alive(cx);
    yukis.sort_by_key(|y| Some(y.entity_id()) == last);
    for y in yukis {
        let (ws, panels) = y.read(cx).session_entry(cx);
        let s = cx.global_mut::<Session>();
        if let Some((id, entry)) = ws {
            s.workspace = Some(id.clone());
            s.workspaces.insert(id, entry);
        }
        [s.sidebar, s.vars_panel, s.git_panel] = panels;
    }
    cx.global::<Session>().write();
}

/// ⌘Q (and a self-update relaunch): nothing unsaved is lost in any window.
pub fn quit(cx: &mut App) {
    for y in alive(cx) {
        y.read(cx).flush_blocking(cx);
    }
    let active = cx.active_window();
    let last = cx
        .global::<Windows>()
        .open
        .iter()
        .find(|(h, _)| Some(*h) == active)
        .map(|(_, y)| y.entity_id());
    save_session(last, cx);
    cx.quit();
}
