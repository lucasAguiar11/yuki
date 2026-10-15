//! A folder (or the workspace) opened as a tab: what its requests inherit —
//! auth, variables, headers — and its notes. Resolved at send time by the
//! engine (`effective_auth`, `apply_folder_vars`, `merge_inherited_metadata`).
//! Edits go to disk 400 ms after typing stops, like requests.

use std::{rc::Rc, sync::Arc, time::Duration};

use gpui_kit::component::{
    h_flex,
    input::{Editor, EditorState, Input, InputEvent, InputState},
    v_flex, ActiveTheme as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::{AuthConfig, EnvironmentVariable, RequestParameter, YukiError};

use crate::{
    auth_form::{self, AuthForm, AuthHost, Owner},
    backend::Backend,
    chip_templates,
    complete::{CompleteHost, Completion, Target},
    complete_view,
    kv::{KvChanged, KvRows},
    secrets, var_view,
    vars::Vars,
};

const AUTOSAVE_DELAY: Duration = Duration::from_millis(400);

/// What a folder/workspace holds for its requests (plaintext secrets).
#[derive(Clone, Default, PartialEq)]
pub struct ScopeData {
    pub auth: AuthConfig,
    pub variables: Vec<EnvironmentVariable>,
    pub headers: Vec<RequestParameter>,
    pub notes: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Auth,
    Variables,
    Headers,
    Notes,
}

/// A write landed: open requests re-resolve their variables.
pub struct ScopeSaved;

pub struct ScopeView {
    backend: Arc<Backend>,
    ws_id: String,
    /// `None`: the workspace.
    pub folder: Option<String>,
    saved: ScopeData,
    form: AuthForm,
    vars: Entity<KvRows>,
    headers: Entity<KvRows>,
    notes: Entity<EditorState>,
    pane: Pane,
    pub focus: FocusHandle,
    error: SharedString,
    autosave: Option<Task<()>>,
    saving: bool,
    /// What `{{ }}` resolves to here (env + folders), for chips and autocomplete.
    resolved: Rc<Vars>,
    env_id: Option<String>,
    complete: Option<Completion>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<ScopeSaved> for ScopeView {}

impl AuthHost for ScopeView {
    fn form(&self) -> &AuthForm {
        &self.form
    }
    fn form_mut(&mut self) -> &mut AuthForm {
        &mut self.form
    }
    fn stored_auth(&self) -> &AuthConfig {
        &self.saved.auth
    }
    fn auth_edited(&mut self, cx: &mut Context<Self>) {
        self.changed(cx);
    }
    fn auth_typed(
        &mut self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        chip_templates!(input, window, cx);
        self.update_completion(input, cx);
    }
    fn decorate(&self, input: Input) -> Input {
        let vars = self.resolved.clone();
        input.token(move |ctx, _, cx| var_view::chip(ctx, &vars, &[], cx).into_any_element())
    }
}

impl CompleteHost for ScopeView {
    fn completion(&self) -> &Option<Completion> {
        &self.complete
    }
    fn completion_mut(&mut self) -> &mut Option<Completion> {
        &mut self.complete
    }
    fn template_vars(&self) -> &Vars {
        &self.resolved
    }
    fn workspace(&self) -> (Arc<Backend>, String) {
        (self.backend.clone(), self.ws_id.clone())
    }
    fn template_fields(&self) -> Vec<Target> {
        self.form.inputs().map(Target::Line).collect()
    }
}

impl Backend {
    /// Secrets opened; `folder: None` is the workspace (no variables, no notes).
    pub fn load_scope(&self, ws: &str, folder: Option<&str>) -> Result<ScopeData, YukiError> {
        let mut data = match folder {
            Some(id) => {
                let f = self.api.requests.get_folder(ws, id)?;
                ScopeData {
                    auth: f.auth,
                    variables: f.variables,
                    headers: f.headers,
                    notes: self.api.requests.get_folder_notes(ws, id)?,
                }
            }
            None => {
                let w = self.api.workspaces.get(ws)?;
                ScopeData {
                    auth: w.auth,
                    headers: w.headers,
                    ..Default::default()
                }
            }
        };
        if let Some(key) = secrets::key(&self.api, ws)? {
            data.auth = secrets::open(data.auth, &key);
            secrets::open_vars(&mut data.variables, &key);
        }
        Ok(data)
    }

    /// Seals secrets on encrypted workspaces; each file is written only if it changed.
    pub fn save_scope(
        &self,
        ws: &str,
        folder: Option<&str>,
        data: &ScopeData,
    ) -> Result<(), YukiError> {
        let key = secrets::key(&self.api, ws)?;
        let Some(id) = folder else {
            let mut w = self.api.workspaces.get(ws)?;
            let auth = match &key {
                Some(k) => secrets::seal(&data.auth, &w.auth, k)?,
                None => data.auth.clone(),
            };
            if w.auth == auth && w.headers == data.headers {
                return Ok(());
            }
            w.auth = auth;
            w.headers = data.headers.clone();
            w.updated_at = chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.6f")
                .to_string();
            return self.api.workspaces.save(&w);
        };
        let store = &self.api.requests;
        let stored = store.get_folder(ws, id)?;
        let (mut auth, mut variables) = (data.auth.clone(), data.variables.clone());
        if let Some(k) = &key {
            auth = secrets::seal(&data.auth, &stored.auth, k)?;
            secrets::seal_vars(&mut variables, &stored.variables, k)?;
        }
        store.update_folder(ws, id, data.headers.clone(), auth)?;
        store.update_folder_variables(ws, id, variables)?;
        store.save_folder_notes(ws, id, &data.notes)
    }
}

impl ScopeView {
    pub fn new(
        backend: Arc<Backend>,
        ws_id: String,
        folder: Option<String>,
        saved: ScopeData,
        env_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let owner = if folder.is_some() {
            Owner::Folder
        } else {
            Owner::Workspace
        };
        let (form, auth_sub) = AuthForm::new(&saved.auth, owner, window, cx);
        let vars = cx.new(|cx| KvRows::variables(&saved.variables, window, cx));
        let headers = cx.new(|cx| KvRows::headers(&saved.headers, window, cx));
        let notes = cx.new(|cx| {
            let mut s = EditorState::new(window, cx)
                .language("markdown")
                .placeholder(t!("markdown notes: what it's for, quirks, example values…"));
            s.set_value(saved.notes.clone(), window, cx);
            s
        });
        let on_kv = |this: &mut Self, _, _: &KvChanged, cx: &mut Context<Self>| this.changed(cx);
        let _subs = vec![
            auth_sub,
            cx.subscribe(&vars, on_kv),
            cx.subscribe(&headers, on_kv),
            cx.subscribe_in(&notes, window, |this, _, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.changed(cx)
                }
            }),
        ];
        let mut this = Self {
            backend,
            ws_id,
            folder,
            saved,
            form,
            vars,
            headers,
            notes,
            pane: Pane::Auth,
            focus: cx.focus_handle(),
            error: "".into(),
            autosave: None,
            saving: false,
            resolved: Rc::default(),
            env_id,
            complete: None,
            _subs,
        };
        this.load_vars(cx);
        this
    }

    pub fn set_env(&mut self, env_id: Option<String>, cx: &mut Context<Self>) {
        self.env_id = env_id;
        self.load_vars(cx);
    }

    /// The variables a request directly in this folder would see.
    fn load_vars(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let (ws, folder, env) = (self.ws_id.clone(), self.folder.clone(), self.env_id.clone());
        let task = cx.background_spawn(async move {
            backend.variables(&ws, folder.as_deref(), env.as_deref())
        });
        cx.spawn(async move |this, cx| {
            let vars = Rc::new(task.await);
            _ = this.update(cx, |this, cx| {
                this.resolved = vars;
                cx.notify();
            });
        })
        .detach();
    }

    fn draft(&self, cx: &App) -> ScopeData {
        ScopeData {
            auth: self
                .form
                .draft(cx)
                .unwrap_or_else(|| self.saved.auth.clone()),
            variables: self.vars.read(cx).to_variables(cx),
            headers: self.headers.read(cx).to_headers(&self.saved.headers, cx),
            notes: self.notes.read(cx).value().to_string(),
        }
    }

    /// The edits not yet on disk, for callers that write before touching files.
    pub fn pending(&self, cx: &App) -> Option<(String, Option<String>, ScopeData)> {
        let draft = self.draft(cx);
        (draft != self.saved).then(|| (self.ws_id.clone(), self.folder.clone(), draft))
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.autosave = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DELAY).await;
            _ = this.update(cx, |s, cx| s.flush(cx));
        }));
        cx.notify();
    }

    /// Write now if anything changed (one write at a time).
    pub fn flush(&mut self, cx: &mut Context<Self>) {
        let draft = self.draft(cx);
        if self.saving || draft == self.saved {
            return;
        }
        self.autosave = None;
        self.saving = true;
        let (backend, ws, folder) = (
            self.backend.clone(),
            self.ws_id.clone(),
            self.folder.clone(),
        );
        let task = cx.background_spawn({
            let draft = draft.clone();
            async move { backend.save_scope(&ws, folder.as_deref(), &draft) }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |s, cx| {
                s.saving = false;
                match result {
                    Ok(()) => {
                        s.error = "".into();
                        s.saved = draft;
                        cx.emit(ScopeSaved);
                        s.load_vars(cx);
                        // Edits typed during the write.
                        s.changed(cx);
                    }
                    Err(e) => s.error = t!("save failed: %{e}", e = e).into_owned().into(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Synchronous write for quit, where a background save would die with the app.
    pub fn flush_blocking(&self, cx: &App) {
        if let Some((ws, folder, draft)) = self.pending(cx) {
            if let Err(e) = self.backend.save_scope(&ws, folder.as_deref(), &draft) {
                eprintln!(
                    "[yuki] failed to save {}: {e}",
                    folder.as_deref().unwrap_or(&ws)
                );
            }
        }
    }

    /// `auth bearer · variables 2 · headers 1 · notes —`
    fn render_meta(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let folder = self.folder.is_some();
        let notes = if self.notes.read(cx).value().trim().is_empty() {
            "notes —"
        } else {
            "notes"
        };
        let mut segments = vec![(Pane::Auth, format!("auth {}", self.form.label()))];
        if folder {
            segments.push((
                Pane::Variables,
                format!("variables {}", self.vars.read(cx).count(cx)),
            ));
        }
        segments.push((
            Pane::Headers,
            format!("headers {}", self.headers.read(cx).count(cx)),
        ));
        if folder {
            segments.push((Pane::Notes, notes.into()));
        }
        let theme = cx.theme();
        let (fg, muted, active_bg, radius) = (
            theme.foreground,
            theme.muted_foreground,
            theme.secondary,
            theme.radius,
        );
        let n = segments.len();
        let items = segments
            .into_iter()
            .enumerate()
            .flat_map(|(ix, (pane, label))| {
                let active = self.pane == pane;
                let seg = div()
                    .id(("scope-pane", ix))
                    .px_1p5()
                    .rounded(radius)
                    .cursor_pointer()
                    .text_color(if active { fg } else { muted })
                    .when(active, |s| s.bg(active_bg))
                    .hover(move |s| s.text_color(fg))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pane = pane;
                        cx.notify();
                    }))
                    .into_any_element();
                let dot = (ix + 1 < n).then(|| div().child("·").into_any_element());
                std::iter::once(seg).chain(dot)
            });
        h_flex()
            .h(px(30.))
            .px_4()
            .gap_1()
            .text_xs()
            .text_color(muted)
            .border_b_1()
            .border_color(cx.theme().border)
            .children(items.collect::<Vec<_>>())
    }
}

impl Render for ScopeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hint = match (self.folder.is_some(), self.pane) {
            (true, Pane::Variables) => t!("{{ NAME }} in the requests of this folder and its subfolders; nearer folders win over environments"),
            (true, _) => t!("requests in this folder and its subfolders inherit this; their own values win"),
            (false, _) => t!("every request in the workspace inherits this; folders and requests override it"),
        };
        let pane = match self.pane {
            Pane::Auth => auth_form::render(self, cx),
            Pane::Variables => self.vars.clone().into_any_element(),
            Pane::Headers => self.headers.clone().into_any_element(),
            Pane::Notes => Editor::new(&self.notes)
                .appearance(false)
                .h_full()
                .into_any_element(),
        };
        let popup = complete_view::render(self, window, cx);
        let theme = cx.theme();
        let root = v_flex()
            .id("scope-view")
            .track_focus(&self.focus)
            .size_full()
            .font_family(theme.mono_font_family.clone());
        complete_view::keys(root, popup.is_some(), cx)
            .child(self.render_meta(cx))
            .child(
                div()
                    .px_4()
                    .pt_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint),
            )
            .when(!self.error.is_empty(), |s| {
                s.child(
                    div()
                        .px_4()
                        .pt_1()
                        .text_xs()
                        .text_color(cx.theme().red)
                        .child(self.error.clone()),
                )
            })
            .child(div().flex_1().min_h_0().px_4().py_2().child(pane))
            .children(popup)
    }
}
