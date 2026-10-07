//! Tree mutations from the sidebar, its menus and the palette: create, rename,
//! duplicate, move, delete. Every one runs on the store off the UI thread and
//! then reloads the tree from disk, so open tabs never drift from the files.

use gpui_kit::component::{button::ButtonVariant, dialog::AlertDialog, WindowExt as _};
use gpui_kit::*;
use yuki_core::{HttpRequest, YukiError};
use yuki_storage::RequestStore;

use crate::{backend::WorkspaceData, tree, workspace::Yuki};

pub(crate) type Op =
    Box<dyn FnOnce(&RequestStore, &str) -> Result<Option<String>, YukiError> + Send>;

impl Yuki {
    fn folder_name(&self, id: &str) -> Option<&str> {
        let d = self.data.as_ref()?;
        d.folders
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.name.as_str())
    }

    pub(crate) fn request(&self, id: &str) -> Option<&HttpRequest> {
        self.data.as_ref()?.requests.iter().find(|r| r.id == id)
    }

    pub(crate) fn is_folder(&self, id: &str) -> bool {
        self.folder_name(id).is_some()
    }

    pub(crate) fn name_of(&self, id: &str) -> Option<String> {
        self.folder_name(id)
            .map(str::to_string)
            .or_else(|| self.request(id).map(|r| r.name.clone()))
    }

    /// Folder of the active request: where ⌘N puts new items.
    pub fn active_folder(&self, cx: &App) -> Option<String> {
        let id = self.active_request_id(cx)?;
        self.request(&id)?.folder_id.clone()
    }

    pub fn active_request_id(&self, cx: &App) -> Option<SharedString> {
        let tab = self.tabs.get(self.active)?;
        Some(tab.read(cx).saved.id.clone().into())
    }

    /// Run `op` on the store, reload the tree, then open (requests) and
    /// optionally rename the item `op` returned.
    pub(crate) fn mutate(
        &mut self,
        rename: bool,
        op: Op,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ws) = self.ws.as_ref().map(|w| w.id.clone()) else {
            return;
        };
        // Flush edits first: duplicate copies what is on disk.
        let drafts: Vec<HttpRequest> = self
            .tabs
            .iter()
            .filter(|t| t.read(cx).dirty)
            .map(|t| t.read(cx).draft(cx))
            .collect();
        let backend = self.backend.clone();
        let task = cx.background_spawn(async move {
            for d in &drafts {
                backend.save(d)?;
            }
            let id = op(&backend.api.requests, &ws)?;
            Ok::<_, YukiError>((id, backend.load(&ws)?))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok((id, data)) => {
                    this.apply_data(data, window, cx);
                    if let Some(id) = id {
                        if this.request(&id).is_some() {
                            this.open_request(&id, window, cx);
                        }
                        if rename {
                            this.start_rename(id.into(), window, cx);
                        }
                    }
                }
                Err(e) => {
                    this.status = format!("falhou: {e}").into();
                    cx.notify();
                }
            })
        })
        .detach();
    }

    /// Swap in fresh tree data: tabs of deleted requests close, the rest take
    /// the on-disk name/folder (a move re-resolves folder variables).
    fn apply_data(&mut self, data: WorkspaceData, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.tabs.len();
        self.tabs
            .retain(|t| data.requests.iter().any(|r| r.id == t.read(cx).saved.id));
        let env = self.env_id(cx);
        for tab in &self.tabs {
            let Some(disk) = data
                .requests
                .iter()
                .find(|r| r.id == tab.read(cx).saved.id)
                .cloned()
            else {
                continue;
            };
            tab.update(cx, |e, cx| {
                let moved = e.saved.folder_id != disk.folder_id;
                e.saved = disk;
                e.refresh_dirty(cx);
                if moved {
                    e.set_env(env.clone(), window, cx);
                }
            });
        }
        self.data = Some(data);
        self.refresh_rows();
        if self.tabs.len() != before {
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    pub fn new_item(
        &mut self,
        folder: bool,
        parent: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(p) = &parent {
            self.collapsed.remove(p);
        }
        let op: Op = Box::new(move |s, ws| {
            Ok(Some(if folder {
                s.create_folder(ws.into(), parent, "nova pasta".into())?.id
            } else {
                s.create_request(
                    ws.into(),
                    parent,
                    "nova request".into(),
                    "GET".into(),
                    String::new(),
                )?
                .id
            }))
        });
        self.mutate(true, op, window, cx);
    }

    pub fn duplicate(&mut self, id: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let (folder, Some(name)) = (self.is_folder(&id), self.name_of(&id)) else {
            return;
        };
        let op: Op = Box::new(move |s, ws| {
            let copy = format!("{name} cópia");
            Ok(Some(if folder {
                let f = s.duplicate_folder(ws, &id)?;
                s.rename_folder(ws, &f.id, copy)?;
                f.id
            } else {
                let r = s.duplicate_request(ws, &id)?;
                s.rename_request(ws, &r.id, copy)?;
                r.id
            }))
        });
        self.mutate(false, op, window, cx);
    }

    /// Asks first: deleting a folder takes everything under it.
    pub fn confirm_delete(
        &mut self,
        id: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(data), Some(name)) = (&self.data, self.name_of(&id)) else {
            return;
        };
        let folder = self.is_folder(&id);
        let detail: SharedString = if folder {
            let n = data
                .requests
                .iter()
                .filter(|r| tree::within(&data.folders, r.folder_id.as_deref(), &id))
                .count();
            match n {
                0 => "a pasta vazia será apagada do disco.".into(),
                1 => "a pasta e a request dentro dela serão apagadas do disco.".into(),
                n => {
                    format!("a pasta e as {n} requests dentro dela serão apagadas do disco.").into()
                }
            }
        } else {
            "a request e o arquivo dela serão apagados do disco.".into()
        };
        let title: SharedString = format!("excluir “{name}”?").into();
        let this = cx.entity().downgrade();
        let build = move |alert: AlertDialog, _: &mut Window, _: &mut App| {
            let (this, id) = (this.clone(), id.clone());
            alert
                .title(title.clone())
                .description(detail.clone())
                .show_cancel(true)
                .ok_text("excluir")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("cancelar")
                .on_ok(move |_, window, cx| {
                    let id = id.to_string();
                    _ = this.update(cx, |this, cx| {
                        let op: Op = Box::new(move |s, ws| {
                            if folder {
                                s.delete_folder_cascade(ws, &id)?;
                            } else {
                                s.delete_request(ws, &id)?;
                            }
                            Ok(None)
                        });
                        this.mutate(false, op, window, cx);
                    });
                    true
                })
        };
        // Deferred: the palette/menu that triggered this restores its own
        // focus afterwards and would pull it out of the dialog. Root focus
        // first, because the dialog gives focus back to whatever held it, and
        // the deleted tab's URL bar would leave the keyboard dead.
        let root = self.focus.clone();
        window.defer(cx, move |window, cx| {
            root.focus(window, cx);
            window.open_alert_dialog(cx, build);
        });
    }
}
