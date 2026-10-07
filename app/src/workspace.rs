use std::{collections::HashSet, sync::Arc};

use gpui_kit::component::{
    h_resizable, resizable_panel,
    searchable_list::SearchableVec,
    select::{SelectEvent, SelectState},
    v_flex, ActiveTheme as _, IndexPath,
};
use gpui_kit::*;
use yuki_core::{EnvironmentKind, Workspace};

use crate::{
    backend::{self, Backend, WorkspaceData},
    chrome,
    editor::{EditorChanged, RequestEditor},
    palette::{Kind, Palette},
    rename::Rename,
    sidebar, tabs, tree, vars_panel, *,
};

type EnvSelect = SelectState<SearchableVec<SharedString>>;

pub struct Yuki {
    pub backend: Arc<Backend>,
    pub(crate) focus: FocusHandle,
    pub workspaces: Vec<Workspace>,
    pub ws: Option<Workspace>,
    pub data: Option<WorkspaceData>,
    pub collapsed: HashSet<String>,
    pub rows: Vec<tree::Row>,
    pub tabs: Vec<Entity<RequestEditor>>,
    pub active: usize,
    sidebar: bool,
    vars_panel: bool,
    /// Show secret values in the variables panel.
    pub reveal: bool,
    pub env: Option<Entity<EnvSelect>>,
    env_ids: Vec<Option<String>>,
    pub(crate) palette: Option<Palette>,
    /// Sidebar row being renamed inline.
    pub renaming: Option<Rename>,
    pub status: SharedString,
    _subs: Vec<Subscription>,
}

impl Yuki {
    pub fn new(backend: Arc<Backend>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            backend,
            focus: cx.focus_handle(),
            workspaces: vec![],
            ws: None,
            data: None,
            collapsed: HashSet::new(),
            rows: vec![],
            tabs: vec![],
            active: 0,
            sidebar: true,
            vars_panel: false,
            reveal: false,
            env: None,
            env_ids: vec![],
            palette: None,
            renaming: None,
            status: "".into(),
            _subs: vec![],
        };
        this.focus.focus(window, cx);
        this.reload_workspaces(window, cx);
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            if let Some(this) = weak.upgrade() {
                this.read(cx).flush_blocking(cx);
            }
            true
        });
        this
    }

    fn reload_workspaces(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let task = cx.background_spawn(async move { backend.workspaces() });
        cx.spawn_in(window, async move |this, cx| {
            let list = task.await;
            this.update_in(cx, |this, window, cx| {
                let first = list.first().cloned();
                this.workspaces = list;
                if this.ws.is_none() {
                    if let Some(ws) = first {
                        this.open_workspace(ws, window, cx);
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    pub fn open_workspace(&mut self, ws: Workspace, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let id = ws.id.clone();
        let task = cx.background_spawn(async move { backend.load(&id) });
        cx.spawn_in(window, async move |this, cx| {
            let data = task.await;
            this.update_in(cx, |this, window, cx| match data {
                Ok(data) => {
                    // Switching never loses edits: flush dirty tabs first.
                    for tab in std::mem::take(&mut this.tabs) {
                        tab.update(cx, |e, cx| e.save(cx));
                    }
                    this._subs.clear();
                    this.active = 0;
                    this.set_envs(&data, window, cx);
                    this.status = format!("{} requests", data.requests.len()).into();
                    this.data = Some(data);
                    this.ws = Some(ws);
                    this.refresh_rows();
                    // The old tab's URL bar held focus; without a focused
                    // element no keybinding fires.
                    this.focus_active(window, cx);
                    cx.notify();
                }
                Err(e) => this.status = format!("Falha ao abrir workspace: {e}").into(),
            })
        })
        .detach();
    }

    fn set_envs(&mut self, data: &WorkspaceData, window: &mut Window, cx: &mut Context<Self>) {
        let personal: Vec<_> = data
            .environments
            .iter()
            .filter(|e| e.kind == EnvironmentKind::Personal)
            .collect();
        let mut names = vec![SharedString::from("Sem ambiente")];
        names.extend(personal.iter().map(|e| SharedString::from(e.name.clone())));
        self.env_ids = std::iter::once(None)
            .chain(personal.iter().map(|e| Some(e.id.clone())))
            .collect();
        // Default to the first personal env: requests usually depend on one.
        let ix = if personal.is_empty() { 0 } else { 1 };
        let env = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(names),
                Some(IndexPath::new(ix)),
                window,
                cx,
            )
        });
        self._subs.push(cx.subscribe_in(
            &env,
            window,
            |this, _, _: &SelectEvent<SearchableVec<SharedString>>, window, cx| {
                // Every open request re-resolves its chips against the new env.
                let env_id = this.env_id(cx);
                for tab in &this.tabs {
                    tab.update(cx, |e, cx| e.set_env(env_id.clone(), window, cx));
                }
            },
        ));
        self.env = Some(env);
    }

    pub(crate) fn env_id(&self, cx: &App) -> Option<String> {
        let ix = self.env.as_ref()?.read(cx).selected_index(cx)?.row;
        self.env_ids.get(ix).cloned().flatten()
    }

    pub fn refresh_rows(&mut self) {
        self.rows = match &self.data {
            Some(d) => tree::rows(&d.folders, &d.requests, &self.collapsed),
            None => vec![],
        };
    }

    pub fn open_request(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.iter().position(|t| t.read(cx).saved.id == id) {
            self.active = ix;
        } else {
            let Some(req) = self
                .data
                .as_ref()
                .and_then(|d| d.requests.iter().find(|r| r.id == id))
                .cloned()
            else {
                return;
            };
            let (backend, env) = (self.backend.clone(), self.env_id(cx));
            let editor = cx.new(|cx| RequestEditor::new(backend, req, env, window, cx));
            self._subs.push(
                cx.subscribe(&editor, |this, editor, _: &EditorChanged, cx| {
                    // Keep the sidebar/palette copy in sync with what's on disk.
                    let saved = editor.read(cx).saved.clone();
                    if let Some(r) = this
                        .data
                        .as_mut()
                        .and_then(|d| d.requests.iter_mut().find(|r| r.id == saved.id))
                    {
                        *r = saved;
                    }
                    this.refresh_rows();
                    cx.notify();
                }),
            );
            self.tabs.push(editor);
            self.active = self.tabs.len() - 1;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    fn active_editor(&self) -> Option<&Entity<RequestEditor>> {
        self.tabs.get(self.active)
    }

    /// Synchronous flush for quit/close, where an async save would die with
    /// the app.
    fn flush_blocking(&self, cx: &App) {
        for tab in &self.tabs {
            let e = tab.read(cx);
            if e.dirty {
                if let Err(err) = self.backend.save(&e.draft(cx)) {
                    eprintln!("[yuki] failed to save {}: {err}", e.saved.id);
                }
            }
        }
    }

    pub fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        // Closing never loses edits: flush a dirty tab first.
        self.tabs[ix].update(cx, |e, cx| e.save(cx));
        self.tabs.remove(ix);
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.active = ix;
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    /// Keyboard-first: the active tab's URL bar owns focus, else the root.
    pub(crate) fn focus_active(&self, window: &mut Window, cx: &mut App) {
        let handle = match self.active_editor() {
            Some(e) => e.read(cx).url_focus(cx),
            None => self.focus.clone(),
        };
        handle.focus(window, cx);
    }

    fn import_voleeo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let task = cx.background_spawn(async move { backend::import_voleeo(&backend::data_dir()) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.status = match result {
                    Ok(n) => format!("{n} workspace(s) importado(s) do Voleeo").into(),
                    Err(e) => format!("Import do Voleeo falhou: {e}").into(),
                };
                this.reload_workspaces(window, cx);
            })
        })
        .detach();
    }

    fn render_center(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.active_editor() {
            Some(editor) => v_flex()
                .size_full()
                .child(tabs::render(self, cx))
                .child(div().flex_1().min_h_0().child(editor.clone()))
                .into_any_element(),
            None => chrome::welcome(!self.workspaces.is_empty(), cx).into_any_element(),
        }
    }

    /// Sidebar | center | vars. Hidden panels are left out, not `.visible(false)`:
    /// a hidden panel keeps a stale size in the group state and the remaining
    /// panels then fight over the gap while dragging (visible flicker). Each
    /// visibility combo gets its own state key so indices never shift.
    fn render_panels(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let id = SharedString::from(format!(
            "main-{}-{}",
            self.sidebar as u8, self.vars_panel as u8
        ));
        let mut group = h_resizable(id);
        if self.sidebar {
            group = group.child(
                resizable_panel()
                    .size(px(240.))
                    .size_range(px(160.)..px(560.))
                    .flex_none()
                    .child(sidebar::render(self, cx)),
            );
        }
        group = group.child(
            resizable_panel()
                .size_range(px(320.)..Pixels::MAX)
                .child(self.render_center(cx)),
        );
        if self.vars_panel {
            group = group.child(
                resizable_panel()
                    .size(px(300.))
                    .size_range(px(220.)..px(600.))
                    .flex_none()
                    .child(vars_panel::render(self, cx)),
            );
        }
        group
    }

    /// `workspace / folder / request` for the title bar.
    pub fn breadcrumb(&self, cx: &App) -> Vec<String> {
        let mut crumbs: Vec<String> = self.ws.iter().map(|w| w.name.clone()).collect();
        if let (Some(editor), Some(data)) = (self.active_editor(), &self.data) {
            let req = &editor.read(cx).saved;
            crumbs.extend(tree::folder_path(&data.folders, req.folder_id.as_deref()));
            crumbs.push(req.name.clone());
        }
        crumbs
    }
}

impl Render for Yuki {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .key_context("Yuki")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Quit, _, cx| {
                this.flush_blocking(cx);
                cx.quit();
            }))
            .on_action(cx.listener(|this, _: &ToggleEditor, _, cx| {
                if let Some(e) = this.active_editor() {
                    e.update(cx, |e, cx| e.toggle_editor(None, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleVars, _, cx| {
                this.vars_panel = !this.vars_panel;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.sidebar = !this.sidebar;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenRequests, window, cx| {
                this.open_palette(Kind::Requests, window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenCommands, window, cx| {
                this.open_palette(Kind::Commands, window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenWorkspaces, window, cx| {
                this.open_palette(Kind::Workspaces, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ImportVoleeo, window, cx| this.import_voleeo(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CloseTab, window, cx| {
                    this.close_tab(this.active, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                let n = this.tabs.len().max(1);
                this.activate_tab((this.active + 1) % n, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PrevTab, window, cx| {
                let n = this.tabs.len().max(1);
                this.activate_tab((this.active + n - 1) % n, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Save, _, cx| {
                if let Some(e) = this.active_editor() {
                    e.update(cx, |e, cx| e.save(cx));
                }
            }))
            .on_action(cx.listener(|this, _: &Send, window, cx| {
                let env = this.env_id(cx);
                if let Some(e) = this.active_editor() {
                    e.update(cx, |e, cx| e.send(env, window, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &NewRequest, window, cx| {
                let parent = this.active_folder(cx);
                this.new_item(false, parent, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NewFolder, window, cx| {
                let parent = this.active_folder(cx);
                this.new_item(true, parent, window, cx)
            }))
            .on_action(cx.listener(|this, _: &RenameItem, window, cx| {
                if let Some(id) = this.active_request_id(cx) {
                    this.sidebar = true;
                    this.start_rename(id, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &DuplicateItem, window, cx| {
                if let Some(id) = this.active_request_id(cx) {
                    this.duplicate(id, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MoveItem, window, cx| {
                if let Some(id) = this.active_request_id(cx) {
                    this.open_palette(Kind::Move(id), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &DeleteItem, window, cx| {
                if let Some(id) = this.active_request_id(cx) {
                    this.confirm_delete(id, window, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &CancelRename, window, cx| this.cancel_rename(window, cx)),
            )
            .child(chrome::title_bar(self, cx))
            .child(div().flex_1().min_h_0().child(self.render_panels(cx)))
            .child(chrome::status_line(self, cx))
            .children(self.palette.as_ref().map(|p| p.render(cx)))
    }
}
