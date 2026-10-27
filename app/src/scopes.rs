//! The folder/workspace tab: one at a time, after the request tabs. Opening
//! another folder saves and replaces it.

use gpui_kit::*;

use crate::{
    scope_view::{ScopeData, ScopeSaved, ScopeView},
    tree,
    workspace::Yuki,
};

impl Yuki {
    /// Open the folder's tab (`None`: the workspace's); `reveal` = a variable
    /// to show there.
    pub fn open_scope(
        &mut self,
        folder: Option<String>,
        reveal: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ws) = self.ws.as_ref().map(|w| w.id.clone()) else {
            return;
        };
        self.close_envs(window, cx);
        self.close_settings(window, cx);
        if let Some((view, _)) = self
            .scope
            .as_ref()
            .filter(|(v, _)| v.read(cx).folder == folder)
        {
            let view = view.clone();
            self.scope_active = true;
            self.focus_active(window, cx);
            if let Some(key) = reveal {
                view.update(cx, |v, cx| v.reveal_var(&key, window, cx));
            }
            cx.notify();
            return;
        }
        let backend = self.backend.clone();
        let task = cx.background_spawn({
            let (backend, ws, folder) = (backend.clone(), ws.clone(), folder.clone());
            async move { backend.load_scope(&ws, folder.as_deref()) }
        });
        cx.spawn_in(window, async move |this, cx| {
            let loaded = task.await;
            _ = this.update_in(cx, |this, window, cx| {
                match loaded {
                    Ok(data) => {
                        if let Some((old, _)) = this.scope.take() {
                            old.update(cx, |v, cx| v.flush(cx));
                        }
                        let env = this.env_id(cx);
                        let view =
                            cx.new(|cx| ScopeView::new(backend, ws, folder, data, env, window, cx));
                        // Folder variables feed every open request's `{{ }}`.
                        let sub =
                            cx.subscribe_in(&view, window, |this, _, _: &ScopeSaved, _, cx| {
                                for tab in &this.tabs {
                                    tab.update(cx, |e, cx| e.load_vars(cx));
                                }
                                this.refresh_git(cx);
                            });
                        if let Some(key) = reveal {
                            view.update(cx, |v, cx| v.reveal_var(&key, window, cx));
                        }
                        this.scope = Some((view, sub));
                        this.scope_active = true;
                        this.focus_active(window, cx);
                    }
                    Err(e) => {
                        this.status = t!("failed to read the folder: %{e}", e = e)
                            .into_owned()
                            .into()
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn close_scope(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((view, _)) = self.scope.take() {
            view.update(cx, |v, cx| v.flush(cx));
        }
        self.scope_active = false;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Unsaved scope edits, for writes that must land before touching files.
    pub(crate) fn scope_pending(&self, cx: &App) -> Option<(String, Option<String>, ScopeData)> {
        self.scope
            .as_ref()
            .and_then(|(v, _)| v.read(cx).pending(cx))
    }

    /// The tab label: the folder's path, or `workspace`.
    pub(crate) fn scope_label(&self, cx: &App) -> Option<String> {
        let (view, _) = self.scope.as_ref()?;
        Some(match &view.read(cx).folder {
            Some(id) => {
                let folders = self.data.as_ref().map_or(&[][..], |d| &d.folders[..]);
                tree::folder_path(folders, Some(id)).join(" / ")
            }
            None => t!("workspace").into_owned(),
        })
    }

    /// Fresh tree data: a deleted folder takes its tab with it.
    pub(crate) fn prune_scope(&mut self, cx: &App) {
        let gone = self.scope.as_ref().is_some_and(|(v, _)| {
            v.read(cx).folder.as_ref().is_some_and(|id| {
                !self
                    .data
                    .as_ref()
                    .is_some_and(|d| d.folders.iter().any(|f| f.id == *id))
            })
        });
        if gone {
            self.scope = None;
            self.scope_active = false;
        }
    }
}
