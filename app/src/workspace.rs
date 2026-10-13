use std::{collections::HashSet, sync::Arc};

use gpui_kit::component::{
    h_resizable, resizable_panel, searchable_list::SearchableVec, select::SelectState, v_flex,
    ActiveTheme as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::Workspace;

use crate::{
    backend::{self, Backend, WorkspaceData},
    chrome,
    editor::{EditorChanged, RequestEditor},
    env_screen::EnvScreen,
    git::Git,
    git_panel,
    palette::{Kind, Palette},
    rename::Rename,
    settings::SettingsScreen,
    sidebar, tabs, tree, vars_panel, *,
};

pub(crate) type EnvSelect = SelectState<SearchableVec<SharedString>>;

pub struct Yuki {
    pub backend: Arc<Backend>,
    pub(crate) focus: FocusHandle,
    pub workspaces: Vec<Workspace>,
    pub ws: Option<Workspace>,
    pub data: Option<WorkspaceData>,
    pub collapsed: HashSet<String>,
    pub rows: Vec<tree::Row>,
    pub tree_scroll: ScrollHandle,
    pub tabs: Vec<Entity<RequestEditor>>,
    pub active: usize,
    sidebar: bool,
    vars_panel: bool,
    git_panel: bool,
    pub git: Git,
    /// Show secret values in the variables panel.
    pub reveal: bool,
    pub env: Option<Entity<EnvSelect>>,
    pub(crate) env_ids: Vec<Option<String>>,
    pub(crate) env_sub: Option<Subscription>,
    /// The "Environments" screen, shown in place of the request while open.
    pub(crate) env_screen: Option<(Entity<EnvScreen>, Subscription)>,
    /// The "Settings" screen, same place; one of the two at a time.
    pub(crate) settings: Option<(Entity<SettingsScreen>, Subscription)>,
    pub(crate) palette: Option<Palette>,
    /// Sidebar row being renamed inline.
    pub renaming: Option<Rename>,
    pub status: SharedString,
    /// Agents connected over MCP; `None` once the server is down.
    pub mcp: Option<usize>,
    _subs: Vec<Subscription>,
}

impl Yuki {
    pub fn new(
        backend: Arc<Backend>,
        mcp: crate::mcp::Link,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            backend,
            focus: cx.focus_handle(),
            workspaces: vec![],
            ws: None,
            data: None,
            collapsed: HashSet::new(),
            rows: vec![],
            tree_scroll: ScrollHandle::new(),
            tabs: vec![],
            active: 0,
            sidebar: true,
            vars_panel: false,
            git_panel: false,
            git: Git::default(),
            reveal: false,
            env: None,
            env_ids: vec![],
            env_sub: None,
            env_screen: None,
            settings: None,
            palette: None,
            renaming: None,
            status: "".into(),
            mcp: Some(0),
            _subs: vec![],
        };
        this.focus.focus(window, cx);
        this.init_git_inputs(window, cx);
        this.reload_workspaces(window, cx);
        this.watch_mcp(mcp, window, cx);
        this.check_update_on_launch(cx);
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
                    if let Some((screen, _)) = this.env_screen.take() {
                        screen.update(cx, |s, cx| s.flush(cx));
                    }
                    if let Some((screen, _)) = this.settings.take() {
                        screen.update(cx, |s, cx| s.flush(cx));
                    }
                    this.set_envs(&data.environments, None, window, cx);
                    this.status = t!("%{n} requests", n = data.requests.len())
                        .into_owned()
                        .into();
                    this.data = Some(data);
                    this.ws = Some(ws);
                    this.git.view = None;
                    this.refresh_git(cx);
                    this.refresh_rows();
                    // The old tab's URL bar held focus; without a focused
                    // element no keybinding fires.
                    this.focus_active(window, cx);
                    cx.notify();
                }
                Err(e) => {
                    this.status = t!("Failed to open workspace: %{error}", error = e)
                        .into_owned()
                        .into()
                }
            })
        })
        .detach();
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
                    // A save changes what git sees.
                    this.refresh_git(cx);
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
    // ponytail: an autosave already in flight can land after this and write
    // the draft from ≤400 ms earlier; a per-request write sequence fixes it if
    // it ever shows up.
    pub(crate) fn flush_blocking(&self, cx: &App) {
        self.flush_envs_blocking(cx);
        if let Some((screen, _)) = &self.settings {
            screen.read(cx).flush_blocking(cx);
        }
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
                    Ok(n) => t!("%{n} workspace(s) imported from Voleeo", n = n)
                        .into_owned()
                        .into(),
                    Err(e) => t!("Voleeo import failed: %{error}", error = e)
                        .into_owned()
                        .into(),
                };
                this.reload_workspaces(window, cx);
            })
        })
        .detach();
    }

    fn render_center(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some((screen, _)) = &self.settings {
            return screen.clone().into_any_element();
        }
        if let Some((screen, _)) = &self.env_screen {
            return screen.clone().into_any_element();
        }
        match self.active_editor() {
            Some(editor) => v_flex()
                .size_full()
                .child(tabs::render(self, cx))
                .child(div().flex_1().min_h_0().child(editor.clone()))
                .into_any_element(),
            None => chrome::welcome(self, cx).into_any_element(),
        }
    }

    /// Sidebar | center | vars. Hidden panels are left out, not `.visible(false)`:
    /// a hidden panel keeps a stale size in the group state and the remaining
    /// panels then fight over the gap while dragging (visible flicker). Each
    /// visibility combo gets its own state key so indices never shift.
    fn render_panels(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let right = if self.git_panel {
            2
        } else {
            self.vars_panel as u8
        };
        let id = SharedString::from(format!("main-{}-{right}", self.sidebar as u8));
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
        if self.vars_panel || self.git_panel {
            let panel = if self.git_panel {
                git_panel::render(self, cx).into_any_element()
            } else {
                vars_panel::render(self, cx).into_any_element()
            };
            group = group.child(
                resizable_panel()
                    .size(px(300.))
                    .size_range(px(220.)..px(600.))
                    .flex_none()
                    .child(panel),
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
            .map(|el| crate::update::on_actions(el, cx))
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
                this.git_panel = false;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleGit, _, cx| {
                this.git_panel = !this.git_panel;
                this.vars_panel = false;
                this.refresh_git(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenEnvs, window, cx| {
                if this.env_screen.is_some() {
                    this.close_envs(window, cx)
                } else {
                    this.open_envs(None, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &CloseEnvs, window, cx| this.close_envs(window, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                if this.settings.is_some() {
                    this.close_settings(window, cx)
                } else {
                    this.open_settings(crate::settings::Section::Workspace, window, cx)
                }
            }))
            .on_action(
                cx.listener(|this, _: &CloseSettings, window, cx| this.close_settings(window, cx)),
            )
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
            .on_action(cx.listener(|this, _: &OpenFolder, window, cx| this.open_folder(window, cx)))
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
            .on_action(cx.listener(|this, _: &CopyAsCurl, _, cx| {
                if let Some(id) = this.active_request_id(cx) {
                    this.copy_as_curl(id, cx);
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
            // Which environment requests go to, readable from anywhere.
            .children(
                self.active_env_color(cx)
                    .map(|c| div().h(px(2.)).w_full().flex_none().bg(c)),
            )
            .child(div().flex_1().min_h_0().child(self.render_panels(cx)))
            .child(chrome::status_line(self, cx))
            .children(self.palette.as_ref().map(|p| p.render(cx)))
    }
}
