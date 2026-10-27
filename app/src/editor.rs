use std::{
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::component::{
    input::{EditorState, InputEvent, InputState},
    searchable_list::SearchableVec,
    select::{SelectEvent, SelectState},
    IndexPath,
};
use gpui_kit::*;
use yuki_core::{BodyKind, HttpRequest, HttpResponse};
use yuki_storage::StoredHttpResponseSummary;

use crate::{
    auth_form::{AuthForm, Owner},
    backend::Backend,
    body::BodyInputs,
    chip_templates,
    complete::{CompleteHost as _, Completion},
    draft,
    params::PathParams,
    response,
    table::{param_field, Table, Which},
    vars,
    vars::Vars,
};

const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// Quiet time after the last edit before it is written.
const AUTOSAVE_DELAY: Duration = Duration::from_millis(400);

pub type MethodSelect = SelectState<SearchableVec<SharedString>>;

#[derive(Clone, Copy, PartialEq)]
pub enum ReqTab {
    Params,
    Headers,
    Body,
    Auth,
    Notes,
    /// The request's file vs the last commit (`req_diff.rs`).
    Diff,
}

#[derive(Clone, Copy, PartialEq)]
pub enum RespTab {
    Body,
    /// Rendered HTML (only offered for `text/html` responses).
    Preview,
    Headers,
    /// What was actually sent + the engine's event log.
    Timeline,
}

/// Emitted when the saved/dirty state changes so the tab bar re-renders.
pub struct EditorChanged;

/// One open request: the prompt line, the collapsible request editor, and the
/// response with its stored history. Rendering lives in `document.rs`.
pub struct RequestEditor {
    pub(crate) backend: Arc<Backend>,
    /// On-disk shape: base, variations, active one.
    pub saved: HttpRequest,
    /// `saved` with its active variation laid over: what the form shows.
    pub shown: HttpRequest,
    pub dirty: bool,
    pub method: Entity<MethodSelect>,
    pub url: Entity<InputState>,
    /// One row per `:name` in the URL (`params.rs`).
    pub params: PathParams,
    /// The URL's query as rows (`params.rs`); the URL stays the saved truth.
    pub query: Table,
    pub headers: Table,
    pub body: BodyInputs,
    pub auth: AuthForm,
    /// Markdown notes (`req_{id}.md`); `notes_saved` is what's on disk.
    pub notes: Entity<EditorState>,
    pub notes_saved: String,
    pub notes_preview: bool,
    /// Git marks this request changed: the meta line offers `diff`.
    pub git_changed: bool,
    /// `git diff` of the request's files; `None` until the pane first opens.
    pub file_diff: Option<SharedString>,
    /// Viewing an older send: when it went out and the request as it was then.
    pub(crate) past: Option<(String, HttpRequest)>,
    pub req_tab: ReqTab,
    pub editor_open: bool,
    /// Collapsed: only the status strip shows (⌘J).
    pub response_open: bool,
    /// Under a binary response: where `save…` put it, or why it failed.
    pub file_note: Option<SharedString>,
    /// The binary response decoded for display (image, PDF pages).
    pub media: Option<crate::binary::Media>,
    pub(crate) media_task: Option<Task<()>>,
    pub sending: bool,
    /// When the in-flight send started (drives the live elapsed counter).
    pub sent_at: Option<Instant>,
    pub response: Option<Result<HttpResponse, String>>,
    /// The shown response's request as sent: literal URL/headers/body.
    pub sent: Option<HttpRequest>,
    /// Focused when the timeline is clicked, so ⌘C copies its selection.
    pub timeline_focus: FocusHandle,
    /// Timeline shows credential headers in full (masked by default).
    pub reveal_tokens: bool,
    pub resp_tab: RespTab,
    pub response_body: Entity<EditorState>,
    pub history: Vec<StoredHttpResponseSummary>,
    /// The shown body parsed as JSON (fields can become variables).
    pub response_json: bool,
    pub(crate) capture: Option<crate::capture_view::Capture>,
    /// The JSON field under the response caret, and the caret it was read at.
    pub(crate) caret: Option<crate::capture_view::CaretField>,
    pub(crate) caret_at: Option<usize>,
    /// Personal env the vars resolve against (globals always apply).
    pub env_id: Option<String>,
    pub vars: Rc<Vars>,
    /// Names/values behind the `$response.body` chips (`vars::Refs`).
    pub refs: Rc<crate::vars::Refs>,
    pub(crate) complete: Option<Completion>,
    /// URL length at the last change: a jump of more than one char is a paste.
    pub(crate) url_len: usize,
    /// Pending autosave; replacing it (next keystroke) cancels the old one.
    pub(crate) autosave: Option<Task<()>>,
    /// A write is in flight: the next one waits, so disk never ends up older.
    pub(crate) saving: bool,
    /// The variation name box (`variation.rs`), when open.
    pub(crate) naming: Option<crate::variation::Naming>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<EditorChanged> for RequestEditor {}
impl EventEmitter<crate::capture_view::VarsChanged> for RequestEditor {}

impl RequestEditor {
    pub fn new(
        backend: Arc<Backend>,
        mut req: HttpRequest,
        env_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // The editor's form: the query lives in the params tab, not the URL bar.
        crate::query::normalize(&mut req);
        // Variations are independent copies: fill what one doesn't hold (older
        // files) and normalize it the same way. In memory only: written once
        // edited, so opening the tab alone never rewrites it.
        req.detach_variations();
        for ix in 0..req.variations.len() {
            let (id, name) = (
                req.variations[ix].id.clone(),
                req.variations[ix].name.clone(),
            );
            let mut eff = HttpRequest {
                active_variation: Some(id.clone()),
                ..req.clone()
            }
            .shown();
            crate::query::normalize(&mut eff);
            req.variations[ix] = yuki_core::Variation::copy_of(&eff, id, name);
        }
        let shown = req.shown();
        let mut methods: Vec<SharedString> =
            METHODS.iter().map(|m| SharedString::from(*m)).collect();
        if !METHODS.contains(&req.method.as_str()) {
            methods.push(req.method.clone().into());
        }
        let ix = methods.iter().position(|m| m.as_ref() == req.method);
        let method = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(methods),
                ix.map(IndexPath::new),
                window,
                cx,
            )
        });
        let url = cx.new(|cx| {
            let mut s = InputState::new(window, cx)
                .placeholder(t!("https://api.example.com/resource").into_owned());
            s.set_value(vars::url_content(&shown.url), window, cx);
            s
        });
        let headers = Table::new(
            Which::Headers,
            shown.headers.iter().map(param_field).collect(),
            window,
            cx,
        );
        let (body, body_subs) = BodyInputs::new(shown.body.as_ref(), window, cx);
        let (auth, auth_sub) = AuthForm::new(&shown.auth, Owner::Request, window, cx);
        let notes = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("markdown")
                .placeholder(t!("markdown notes: what it's for, quirks, example values…"))
        });
        let response_body = cx.new(|cx| EditorState::new(window, cx).language("json"));

        let mut _subs = vec![
            cx.subscribe_in(
                &method,
                window,
                |this, _, _: &SelectEvent<SearchableVec<SharedString>>, _, cx| {
                    this.refresh_dirty(cx)
                },
            ),
            cx.subscribe_in(&url, window, |this, url, ev: &InputEvent, window, cx| {
                match ev {
                    InputEvent::Change => {
                        chip_templates!(url, window, cx);
                        crate::var_view::chip_path_params(url, window, cx);
                        this.update_completion(url, cx);
                        this.sync_path_params(window, cx);
                        let len = url.read(cx).value().len();
                        let pasted = len > this.url_len + 1;
                        this.url_len = len;
                        if pasted {
                            let text = url.read(cx).value().to_string();
                            if text.trim_start().starts_with("curl ") {
                                this.change(|r| _ = crate::paste_curl::apply(&text, r), window, cx);
                                return;
                            }
                            this.take_url_query(window, cx);
                        }
                        this.refresh_dirty(cx);
                    }
                    // A query typed by hand moves once the URL is left.
                    InputEvent::Blur => this.take_url_query(window, cx),
                    _ => {}
                }
            }),
        ];
        _subs.extend(body_subs);
        _subs.push(auth_sub);
        _subs.push(cx.subscribe_in(&notes, window, Self::on_input));
        _subs.push(cx.observe(&response_body, |this, _, cx| this.track_caret(cx)));
        let has_body = shown
            .body
            .as_ref()
            .is_some_and(|b| b.kind != BodyKind::None);
        let was_path = yuki_mcp::resolve::path_params(&shown.url);
        let query_rows = shown
            .parameters
            .iter()
            .filter(|p| !was_path.contains(&p.name))
            .map(param_field)
            .collect();
        let query = Table::new(Which::Query, query_rows, window, cx);
        let url_len = shown.url.len();
        let mut this = Self {
            backend,
            saved: req,
            shown,
            dirty: false,
            method,
            url,
            params: PathParams::default(),
            query,
            auth,
            notes,
            notes_saved: String::new(),
            notes_preview: false,
            git_changed: false,
            file_diff: None,
            past: None,
            headers,
            body,
            req_tab: if has_body {
                ReqTab::Body
            } else {
                ReqTab::Headers
            },
            editor_open: has_body,
            response_open: true,
            file_note: None,
            media: None,
            media_task: None,
            sending: false,
            sent_at: None,
            response: None,
            sent: None,
            resp_tab: RespTab::Body,
            response_body,
            timeline_focus: cx.focus_handle(),
            reveal_tokens: false,
            history: vec![],
            response_json: false,
            capture: None,
            caret: None,
            caret_at: None,
            env_id,
            vars: Rc::default(),
            refs: Rc::default(),
            complete: None,
            url_len,
            autosave: None,
            saving: false,
            naming: None,
            _subs,
        };
        this.sync_path_params(window, cx);
        this.refresh_history(true, window, cx);
        this.load_vars(cx);
        this.load_notes(window, cx);
        this
    }

    pub(crate) fn on_input<E>(
        &mut self,
        _: &Entity<E>,
        ev: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(ev, InputEvent::Change) {
            self.refresh_dirty(cx);
        }
    }

    pub fn url_focus(&self, cx: &App) -> FocusHandle {
        self.url.read(cx).focus_handle(cx)
    }

    /// What the editor holds, on disk's shape: edits to the active variation
    /// land in it, the base untouched.
    pub fn draft(&self, cx: &App) -> HttpRequest {
        draft::fold(&self.saved, self.edited(cx))
    }

    /// The request as the form shows it (active variation applied, none kept).
    pub fn edited(&self, cx: &App) -> HttpRequest {
        let method = self
            .method
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_else(|| "GET".into());
        let url_text = self.url.read(cx).value();
        let mut req = draft::build(
            &self.shown,
            &method,
            &crate::query::without_query(&url_text),
            self.headers.params(cx),
            &self.body.draft(cx),
        );
        // Path params, then the query rows, then a query still typed in the URL.
        let was_path = yuki_mcp::resolve::path_params(&self.shown.url);
        let saved_path: Vec<_> = (self.shown.parameters.iter())
            .filter(|p| was_path.contains(&p.name))
            .cloned()
            .collect();
        req.parameters =
            draft::merge_path_params(&self.shown.url, &saved_path, &self.params.values(cx));
        req.parameters.extend(self.query.params(cx));
        req.parameters
            .extend(crate::query::as_params(crate::query::pairs(&url_text)));
        if let Some(auth) = self.auth.draft(cx) {
            req.auth = auth;
        }
        req
    }

    pub(crate) fn refresh_dirty(&mut self, cx: &mut Context<Self>) {
        let dirty = self.draft(cx) != self.saved || self.notes_text(cx) != self.notes_saved;
        if dirty != self.dirty {
            self.dirty = dirty;
            cx.emit(EditorChanged);
            cx.notify();
        }
        // Local-first: edits reach disk shortly after typing stops.
        self.autosave = dirty.then(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(AUTOSAVE_DELAY).await;
                _ = this.update(cx, |e, cx| e.save(cx));
            })
        });
    }

    pub fn toggle_editor(&mut self, tab: Option<ReqTab>, cx: &mut Context<Self>) {
        match tab {
            Some(t) if !self.editor_open || self.req_tab != t => {
                self.req_tab = t;
                self.editor_open = true;
            }
            _ => self.editor_open = !self.editor_open,
        }
        // Both folded would leave an empty page: the response takes it back.
        self.response_open |= !self.editor_open;
        cx.notify();
    }

    pub fn toggle_response(&mut self, cx: &mut Context<Self>) {
        self.response_open = !self.response_open;
        // The editor fills the room the response gave up.
        self.editor_open |= !self.response_open;
        cx.notify();
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        // In flight: its `mark_saved` re-checks dirty and schedules the rest.
        if !self.dirty || self.saving {
            return;
        }
        self.saving = true;
        let req = self.draft(cx);
        let notes = self.notes_text(cx);
        let backend = self.backend.clone();
        let task = cx.background_spawn({
            let (req, notes) = (req.clone(), notes.clone());
            async move {
                backend.save(&req)?;
                backend.save_notes(&req.workspace_id, &req.id, &notes)
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => this.mark_saved(req, notes, cx),
                    Err(e) => {
                        this.response =
                            Some(Err(t!("failed to save: %{error}", error = e).into_owned()))
                    }
                }
            })
        })
        .detach();
    }

    fn mark_saved(&mut self, req: HttpRequest, notes: String, cx: &mut Context<Self>) {
        self.shown = req.shown();
        self.saved = req;
        self.notes_saved = notes;
        // A response chip added since the last read: show its source and value.
        let refs = &self.refs;
        if (self.used_vars(cx).iter())
            .any(|e| yuki_mcp::resolve::response_args(e).is_some() && !refs.values.contains_key(e))
        {
            self.load_vars(cx);
        }
        self.refresh_dirty(cx);
        cx.emit(EditorChanged);
    }

    pub fn send(&mut self, env_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.sending {
            self.backend.cancel(&self.saved.id);
            return;
        }
        let req = self.draft(cx);
        self.sending = true;
        self.sent_at = Some(Instant::now());
        cx.notify();
        // Re-render the elapsed counter until the send settles.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let sending = this.update(cx, |e, cx| {
                cx.notify();
                e.sending
            });
            if !matches!(sending, Ok(true)) {
                break;
            }
        })
        .detach();
        let backend = self.backend.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = backend.save_and_send(req.clone(), env_id).await;
            this.update_in(cx, |this, window, cx| {
                this.sending = false;
                this.past = None;
                let notes = this.notes_saved.clone();
                this.mark_saved(req, notes, cx);
                let resp_id = result.as_ref().map(|r| r.response_id.clone()).ok();
                this.sent = None;
                this.show_response(result, window, cx);
                if let Some(id) = resp_id.filter(|id| !id.is_empty()) {
                    this.load_sent(id, window, cx);
                }
                this.refresh_history(false, window, cx);
                // Variables reading this response (`$response.body`) changed.
                cx.emit(crate::capture_view::VarsChanged);
            })
        })
        .detach();
    }

    /// Elapsed time of the in-flight send: `1.2 s`, `340 ms`.
    pub fn elapsed(&self) -> String {
        let ms = self.sent_at.map_or(0, |t| t.elapsed().as_millis());
        if ms < 1000 {
            format!("{ms} ms")
        } else {
            format!("{:.1} s", ms as f64 / 1000.)
        }
    }

    fn load_sent(&mut self, resp_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let (ws, id) = (self.saved.workspace_id.clone(), self.saved.id.clone());
        let task = cx.background_spawn(async move { backend.sent_request(&ws, &id, &resp_id) });
        cx.spawn_in(window, async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                this.sent = sent;
                cx.notify();
            })
        })
        .detach();
    }

    /// Reload stored history; `show_latest` also displays the newest entry
    /// (opening a request shows its last response).
    fn refresh_history(&mut self, show_latest: bool, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let (ws, id) = (self.saved.workspace_id.clone(), self.saved.id.clone());
        let task = cx.background_spawn(async move {
            let history = backend.history(&ws, &id);
            let latest = show_latest
                .then(|| {
                    history
                        .first()
                        .and_then(|s| backend.stored_response(&ws, &id, &s.id).ok())
                })
                .flatten();
            (history, latest)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (history, latest) = task.await;
            this.update_in(cx, |this, window, cx| {
                this.history = history;
                if let Some((resp, sent)) = latest {
                    this.sent = sent;
                    this.show_response(Ok(resp), window, cx);
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Another tab's send may have sent this request too (it was read through
    /// `$response.body`): pick up a newer response if one landed.
    pub(crate) fn sync_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let newest = self.history.first().map(|s| s.id.clone());
        let backend = self.backend.clone();
        let (ws, id) = (self.saved.workspace_id.clone(), self.saved.id.clone());
        let task = cx.background_spawn(async move {
            let history = backend.history(&ws, &id);
            let latest = history
                .first()
                .filter(|s| Some(&s.id) != newest.as_ref())
                .and_then(|s| backend.stored_response(&ws, &id, &s.id).ok());
            (history, latest)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (history, latest) = task.await;
            this.update_in(cx, |this, window, cx| {
                if let Some((resp, sent)) = latest {
                    this.history = history;
                    this.sent = sent;
                    this.show_response(Ok(resp), window, cx);
                }
            })
        })
        .detach();
    }

    pub fn show_history(&mut self, resp_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let (ws, id) = (self.saved.workspace_id.clone(), self.saved.id.clone());
        // An older send also shows the request it came from (`past.rs`).
        let summary = self
            .history
            .iter()
            .position(|s| s.id == resp_id)
            .and_then(|ix| (ix > 0).then(|| crate::ui::clock(&self.history[ix].recorded_at)));
        let task = cx.background_spawn(async move {
            let past =
                summary.and_then(|at| Some((at, backend.edited_request(&ws, &id, &resp_id)?)));
            (backend.stored_response(&ws, &id, &resp_id), past)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (result, past) = task.await;
            this.update_in(cx, |this, window, cx| {
                this.past = past;
                match result.map_err(|e| e.to_string()) {
                    Ok((resp, sent)) => {
                        this.sent = sent;
                        this.show_response(Ok(resp), window, cx)
                    }
                    Err(e) => {
                        this.sent = None;
                        this.show_response(Err(e), window, cx)
                    }
                }
            })
        })
        .detach();
    }

    fn show_response(
        &mut self,
        result: Result<HttpResponse, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Ok(resp) = &result {
            let (text, lang) = response::body_for_editor(resp);
            self.response_json = lang == "json";
            self.response_body.update(cx, |s, cx| {
                s.set_highlighter(lang, cx);
                s.set_value(text, window, cx);
            });
        }
        self.capture = None;
        self.file_note = None;
        self.caret = None;
        self.caret_at = None;
        self.response = Some(result);
        self.load_media(cx);
        cx.notify();
    }

    pub fn header_count(&self, cx: &App) -> usize {
        self.headers.active_count(cx)
    }
}
