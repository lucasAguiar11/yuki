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
    backend::Backend,
    body::BodyInputs,
    chip_templates,
    complete::Completion,
    draft, response,
    table::{param_field, Table, Which},
    vars,
    vars::Vars,
};

const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

pub type MethodSelect = SelectState<SearchableVec<SharedString>>;

#[derive(Clone, Copy, PartialEq)]
pub enum ReqTab {
    Headers,
    Body,
}

#[derive(Clone, Copy, PartialEq)]
pub enum RespTab {
    Body,
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
    pub saved: HttpRequest,
    pub dirty: bool,
    pub method: Entity<MethodSelect>,
    pub url: Entity<InputState>,
    pub headers: Table,
    pub body: BodyInputs,
    pub req_tab: ReqTab,
    pub editor_open: bool,
    pub sending: bool,
    /// When the in-flight send started (drives the live elapsed counter).
    pub sent_at: Option<Instant>,
    pub response: Option<Result<HttpResponse, String>>,
    /// The shown response's request as sent: literal URL/headers/body.
    pub sent: Option<HttpRequest>,
    /// Focused when the timeline is clicked, so ⌘C copies its selection.
    pub timeline_focus: FocusHandle,
    pub resp_tab: RespTab,
    pub response_body: Entity<EditorState>,
    pub history: Vec<StoredHttpResponseSummary>,
    /// Personal env the vars resolve against (globals always apply).
    pub env_id: Option<String>,
    pub vars: Rc<Vars>,
    pub(crate) complete: Option<Completion>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<EditorChanged> for RequestEditor {}

impl RequestEditor {
    pub fn new(
        backend: Arc<Backend>,
        req: HttpRequest,
        env_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
            let mut s = InputState::new(window, cx).placeholder("https://api.exemplo.com/recurso");
            s.set_value(vars::content(&req.url), window, cx);
            s
        });
        let headers = Table::new(
            Which::Headers,
            req.headers.iter().map(param_field).collect(),
            window,
            cx,
        );
        let (body, body_subs) = BodyInputs::new(req.body.as_ref(), window, cx);
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
                if matches!(ev, InputEvent::Change) {
                    chip_templates!(url, window, cx);
                    this.update_completion(url, cx);
                    this.refresh_dirty(cx);
                }
            }),
        ];
        _subs.extend(body_subs);
        let has_body = req.body.as_ref().is_some_and(|b| b.kind != BodyKind::None);
        let mut this = Self {
            backend,
            saved: req,
            dirty: false,
            method,
            url,
            headers,
            body,
            req_tab: if has_body {
                ReqTab::Body
            } else {
                ReqTab::Headers
            },
            editor_open: has_body,
            sending: false,
            sent_at: None,
            response: None,
            sent: None,
            resp_tab: RespTab::Body,
            response_body,
            timeline_focus: cx.focus_handle(),
            history: vec![],
            env_id,
            vars: Rc::default(),
            complete: None,
            _subs,
        };
        this.refresh_history(true, window, cx);
        this.load_vars(window, cx);
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

    pub fn draft(&self, cx: &App) -> HttpRequest {
        let method = self
            .method
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_else(|| "GET".into());
        draft::build(
            &self.saved,
            &method,
            &self.url.read(cx).value(),
            self.headers.params(cx),
            &self.body.draft(cx),
        )
    }

    pub(crate) fn refresh_dirty(&mut self, cx: &mut Context<Self>) {
        let dirty = self.draft(cx) != self.saved;
        if dirty != self.dirty {
            self.dirty = dirty;
            cx.emit(EditorChanged);
            cx.notify();
        }
    }

    pub fn toggle_editor(&mut self, tab: Option<ReqTab>, cx: &mut Context<Self>) {
        match tab {
            Some(t) if !self.editor_open || self.req_tab != t => {
                self.req_tab = t;
                self.editor_open = true;
            }
            _ => self.editor_open = !self.editor_open,
        }
        cx.notify();
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        if !self.dirty {
            return;
        }
        let req = self.draft(cx);
        let backend = self.backend.clone();
        let task = cx.background_spawn({
            let req = req.clone();
            async move { backend.save(&req) }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| match result {
                Ok(()) => this.mark_saved(req, cx),
                Err(e) => this.response = Some(Err(format!("Falha ao salvar: {e}"))),
            })
        })
        .detach();
    }

    fn mark_saved(&mut self, req: HttpRequest, cx: &mut Context<Self>) {
        self.saved = req;
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
                this.mark_saved(req, cx);
                let resp_id = result.as_ref().map(|r| r.response_id.clone()).ok();
                this.sent = None;
                this.show_response(result, window, cx);
                if let Some(id) = resp_id.filter(|id| !id.is_empty()) {
                    this.load_sent(id, window, cx);
                }
                this.refresh_history(false, window, cx);
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

    pub fn show_history(&mut self, resp_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let (ws, id) = (self.saved.workspace_id.clone(), self.saved.id.clone());
        let task = cx.background_spawn(async move { backend.stored_response(&ws, &id, &resp_id) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await.map_err(|e| e.to_string());
            this.update_in(cx, |this, window, cx| match result {
                Ok((resp, sent)) => {
                    this.sent = sent;
                    this.show_response(Ok(resp), window, cx)
                }
                Err(e) => {
                    this.sent = None;
                    this.show_response(Err(e), window, cx)
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
            self.response_body.update(cx, |s, cx| {
                s.set_highlighter(lang, cx);
                s.set_value(text, window, cx);
            });
        }
        self.response = Some(result);
        cx.notify();
    }

    pub fn header_count(&self, cx: &App) -> usize {
        self.headers.active_count(cx)
    }
}
