//! MCP: the engine's tool server listens on `mcp.sock` in the data dir while the
//! app runs; `yuki mcp` is the stdio bridge an MCP client launches; agent edits
//! to the open workspace refresh the tree and the env list.

use std::{fs, io, path::PathBuf, sync::Arc};

use gpui_kit::*;
use tokio::sync::{broadcast, watch, RwLock};
use yuki_mcp::api::NotifyFn;

use crate::{backend, backend::Backend, workspace::Yuki};

fn socket() -> PathBuf {
    backend::data_dir().join("mcp.sock")
}

fn token_path() -> PathBuf {
    backend::data_dir().join("mcp-token")
}

/// Made on first launch and read by the bridge, so client configs carry no
/// secret. Owner-only, like the socket.
fn token() -> io::Result<String> {
    if let Ok(t) = fs::read_to_string(token_path()) {
        if !t.trim().is_empty() {
            return Ok(t.trim().to_owned());
        }
    }
    let token: String = yuki_crypto::generate_key()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    #[cfg(unix)]
    {
        use std::{io::Write as _, os::unix::fs::OpenOptionsExt as _};
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(token_path())?
            .write_all(token.as_bytes())?;
    }
    #[cfg(not(unix))]
    fs::write(token_path(), &token)?;
    Ok(token)
}

/// What a window listens to: agent edits and how many agents are connected.
pub struct Link {
    pub edits: broadcast::Receiver<Edit>,
    pub clients: watch::Receiver<usize>,
}

/// What changed on disk: MCP tools fire these, and so does returning to a
/// window (`windows::reload`), for files edited outside the app.
#[derive(Clone)]
pub enum Edit {
    /// Requests, folders or envs of this workspace.
    Items(String),
    /// The workspace list, or a workspace's own fields (auth).
    Workspaces,
}

/// The returned receiver's `changed()` fails once the server is down.
pub fn start(backend: &Backend) -> watch::Receiver<usize> {
    let (clients, rx) = watch::channel(0);
    let token = match token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[mcp] no token at {}: {e}", token_path().display());
            return rx;
        }
    };
    backend.rt.spawn(yuki_mcp::server::run(
        socket(),
        backend.api.clone(),
        Arc::new(RwLock::new(Some(token))),
        Arc::new(RwLock::new(true)),
        clients,
    ));
    rx
}

/// `yuki mcp`: pipe stdio to the running app.
#[cfg(unix)]
pub fn bridge() -> io::Result<()> {
    let token = fs::read_to_string(token_path()).map_err(|e| {
        let path = token_path();
        io::Error::new(
            e.kind(),
            format!("{}: {e} — open Yuki once", path.display()),
        )
    })?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = rt.block_on(yuki_mcp::bridge::run(&socket(), token.trim()));
    // The stdin reader sits on a blocking thread; don't wait for it.
    rt.shutdown_background();
    result
}

/// The engine's notify hook, narrowed to the edits the windows show; each
/// window subscribes to the sender.
pub fn changes() -> (NotifyFn, broadcast::Sender<Edit>) {
    let (tx, _) = broadcast::channel(64);
    let sender = tx.clone();
    let notify: NotifyFn = Arc::new(move |event, payload| {
        let edit = match event {
            "mcp:workspaces:changed" => Edit::Workspaces,
            "mcp:requests:changed" | "mcp:envs:changed" => match payload["workspaceId"].as_str() {
                Some(ws) => Edit::Items(ws.to_owned()),
                None => return,
            },
            _ => return,
        };
        // No window subscribed yet: nobody to refresh.
        let _ = tx.send(edit);
    });
    (notify, sender)
}

impl Yuki {
    pub(crate) fn watch_mcp(&self, link: Link, window: &mut Window, cx: &mut Context<Self>) {
        let Link { edits, mut clients } = link;
        cx.spawn(async move |this, cx| loop {
            let up = clients.changed().await.is_ok();
            let n = up.then(|| *clients.borrow_and_update());
            let alive = this.update(cx, |this, cx| {
                this.mcp = n;
                cx.notify();
            });
            if !up || alive.is_err() {
                break;
            }
        })
        .detach();
        self.watch_agent_edits(edits, window, cx);
    }

    /// Tree, envs and count reload; open tabs follow (`reload_clean_tabs`).
    /// Workspace edits reload the list (and the open one's copy).
    fn watch_agent_edits(
        &self,
        mut changes: broadcast::Receiver<Edit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let edit = match changes.recv().await {
                    Ok(edit) => edit,
                    // Fell behind a burst: reload whatever this window shows.
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        match this.update(cx, |this, _| this.ws.as_ref().map(|w| w.id.clone())) {
                            Ok(Some(id)) => Edit::Items(id),
                            Ok(None) => continue,
                            Err(_) => break,
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let ws_id = match edit {
                    Edit::Items(id) => id,
                    Edit::Workspaces => {
                        let alive = this
                            .update_in(cx, |this, window, cx| this.reload_workspaces(window, cx));
                        if alive.is_err() {
                            break;
                        }
                        continue;
                    }
                };
                let backend = match this.update(cx, |this, _| {
                    let open = this.ws.as_ref().is_some_and(|w| w.id == ws_id);
                    open.then(|| this.backend.clone())
                }) {
                    Ok(Some(b)) => b,
                    Ok(None) => continue,
                    Err(_) => break,
                };
                let load = cx
                    .background_executor()
                    .spawn(async move { backend.load(&ws_id).map(|d| (ws_id, d)) });
                let Ok((ws_id, data)) = load.await else {
                    continue;
                };
                let alive = this.update_in(cx, |this, window, cx| {
                    if !this.ws.as_ref().is_some_and(|w| w.id == ws_id) {
                        return;
                    }
                    this.set_envs(&data.environments, None, window, cx);
                    this.data = Some(data);
                    this.reload_clean_tabs(window, cx);
                    this.refresh_rows();
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// After an agent's edit: tabs of deleted requests close, and a tab with no
    /// unsaved edits shows its request as it is on disk now. A tab with edits
    /// keeps them (its next save wins).
    fn reload_clean_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(requests) = self.data.as_ref().map(|d| d.requests.clone()) else {
            return;
        };
        let before = self.tabs.len();
        self.tabs
            .retain(|t| requests.iter().any(|r| r.id == t.read(cx).saved.id));
        for tab in self.tabs.clone() {
            let id = tab.read(cx).saved.id.clone();
            let Some(disk) = requests.iter().find(|r| r.id == id).cloned() else {
                continue;
            };
            tab.update(cx, |e, cx| {
                let busy = e.dirty || e.sending || e.saving;
                if busy || e.saved.updated_at == disk.updated_at {
                    return;
                }
                let refocus = e.url_focus(cx).is_focused(window);
                e.rebuild(disk, window, cx);
                if refocus {
                    e.url_focus(cx).focus(window, cx);
                }
            });
        }
        if self.tabs.len() != before {
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
            self.focus_active(window, cx);
        }
    }
}
